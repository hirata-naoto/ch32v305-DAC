use embassy_futures::yield_now;

use ch32_hal::pac;

const I2S_CLOCK_HZ: u32 = 144_000_000;

pub struct I2s2Tx;

impl I2s2Tx {
    pub fn new() -> Self {
        enable_clocks();
        configure_gpio_port_b_pin_af(12);
        configure_gpio_port_b_pin_af(13);
        configure_gpio_port_b_pin_af(15);
        Self
    }

    pub fn configure(&mut self, sample_rate_hz: u32) {
        let divider = calculate_i2s_divider(sample_rate_hz);
        let prescaler = divider / 2;
        let odd = (divider & 0x01) != 0;

        pac::SPI2.ctlr1().modify(|w| w.set_spe(false));
        pac::SPI2.i2s_cfgr().modify(|w| w.set_i2se(false));

        pac::SPI2.i2spr().write(|w| {
            w.set_mckoe(false);
            w.set_odd(odd);
            w.set_i2sdiv(prescaler as u8);
        });

        pac::SPI2.i2s_cfgr().write(|w| {
            w.set_chlen(pac::spi::vals::Chlen::BIT16);
            w.set_datlen(pac::spi::vals::I2sdatlen::BIT16);
            w.set_ckpol(false);
            w.set_i2sstd(pac::spi::vals::I2sstd::PHILIPS);
            w.set_pcmsync(false);
            w.set_i2scfg(pac::spi::vals::I2scfg::MASTERTX);
            w.set_i2se(true);
            w.set_i2smod(true);
        });
    }

    pub async fn write_packet(&mut self, data: &[u8]) {
        for sample_bytes in data.chunks_exact(2) {
            while !pac::SPI2.statr().read().txe() {}
            let sample = u16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
            pac::SPI2.datar().write(|w| w.set_datar(sample));
        }

        while pac::SPI2.statr().read().bsy() {}
        yield_now().await;
    }
}

fn enable_clocks() {
    pac::RCC.apb2pcenr().modify(|w| {
        w.set_afioen(true);
        w.set_iopben(true);
    });
    pac::RCC.apb1pcenr().modify(|w| w.set_spi2en(true));
    pac::RCC.apb1prstr().modify(|w| w.set_spi2rst(true));
    pac::RCC.apb1prstr().modify(|w| w.set_spi2rst(false));
}

fn configure_gpio_port_b_pin_af(pin: usize) {
    use pac::gpio::vals::{Cnf, Mode};

    if pin < 8 {
        pac::GPIOB.cfglr().modify(|w| {
            w.set_mode(pin, Mode::OUTPUT_50MHZ);
            w.set_cnf(pin, Cnf::PULL_IN__AF_PUSH_PULL_OUT);
        });
    } else {
        pac::GPIOB.cfghr().modify(|w| {
            w.set_mode(pin - 8, Mode::OUTPUT_50MHZ);
            w.set_cnf(pin - 8, Cnf::PULL_IN__AF_PUSH_PULL_OUT);
        });
    }
}

fn calculate_i2s_divider(sample_rate_hz: u32) -> u16 {
    let base = 32 * sample_rate_hz;
    let divider = ((I2S_CLOCK_HZ + (base / 2)) / base).clamp(4, 510);
    divider as u16
}
