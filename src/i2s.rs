use ch32_hal::dma::{Channel, Priority, TransferOptions, WritableRingBuffer};
use ch32_hal::{pac, Peri};

const I2S_CLOCK_HZ: u32 = 144_000_000;
const I2S_FRAME_BITS: u32 = 64;

pub struct I2s2Tx<'d> {
    dma_ring: WritableRingBuffer<'d, u16>,
    started: bool,
}

impl<'d> I2s2Tx<'d> {
    pub fn new<DmaCh: Channel>(tx_dma: Peri<'d, DmaCh>, dma_buffer: &'d mut [u16]) -> Self {
        // SPI2 を I2S2 送信に使えるようクロックと GPIO を初期化する。
        enable_clocks();
        configure_gpio_port_b_pin_af(12);
        configure_gpio_port_b_pin_af(13);
        configure_gpio_port_b_pin_af(15);

        let mut dma_options = TransferOptions::default();
        dma_options.priority = Priority::VeryHigh;

        let dma_ring = unsafe {
            // SPI2 のデータレジスタは 16bit 幅なので、32bit PCM も半語 2 個ずつ順番に流し込む。
            WritableRingBuffer::new(
                tx_dma,
                (),
                pac::SPI2.datar().as_ptr() as *mut u16,
                dma_buffer,
                dma_options,
            )
        };

        Self {
            dma_ring,
            started: false,
        }
    }

    pub fn configure(&mut self, sample_rate_hz: u32) {
        let divider = calculate_i2s_divider(sample_rate_hz);
        let prescaler = divider / 2;
        let odd = (divider & 0x01) != 0;

        // 設定変更前に SPI/I2S を停止して分周値とフォーマットを更新する。
        pac::SPI2.ctlr2().modify(|w| w.set_txdmaen(false));
        pac::SPI2.ctlr1().modify(|w| w.set_spe(false));
        pac::SPI2.i2s_cfgr().modify(|w| w.set_i2se(false));

        pac::SPI2.i2spr().write(|w| {
            w.set_mckoe(false);
            w.set_odd(odd);
            w.set_i2sdiv(prescaler as u8);
        });

        pac::SPI2.i2s_cfgr().write(|w| {
            // 32-bit stereo を 1 チャンネル 32bit 幅で送るため、I2S フレーム長と有効データ長を両方 32bit にする。
            w.set_chlen(pac::spi::vals::Chlen::BIT32);
            w.set_datlen(pac::spi::vals::I2sdatlen::BIT32);
            w.set_ckpol(false);
            w.set_i2sstd(pac::spi::vals::I2sstd::PHILIPS);
            w.set_pcmsync(false);
            w.set_i2scfg(pac::spi::vals::I2scfg::MASTERTX);
            w.set_i2se(true);
            w.set_i2smod(true);
        });

        self.started = false;
    }

    pub fn prime(&mut self, data: &[u16]) {
        // DMA 開始前に無音や先頭データを入れて、開始直後のアンダーランを避ける。
        self.dma_ring.clear();
        let _ = self.dma_ring.write_immediate(data);
    }

    pub fn start(&mut self) {
        if self.started {
            return;
        }

        // I2S 本体と DMA リクエストを同時に有効化して連続送信を始める。
        pac::SPI2.ctlr1().modify(|w| w.set_spe(true));
        pac::SPI2.ctlr2().modify(|w| w.set_txdmaen(true));
        self.dma_ring.start();
        self.started = true;
    }

    pub async fn write_words(&mut self, data: &[u16]) {
        // DMA リングへ書けるぶんだけ先回りで埋め、CPU のポーリング送信をなくす。
        if self.dma_ring.write_exact(data).await.is_err() {
            // 万一 DMA 側へ追いつけなかった場合は、現在の位置に合わせて書き込みを立て直す。
            self.dma_ring.clear();
            let _ = self.dma_ring.write_immediate(data);
        }
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
    let base = I2S_FRAME_BITS * sample_rate_hz;
    // 32-bit stereo のビットクロックに合わせて最も近い整数分周値を選ぶ。
    let divider = ((I2S_CLOCK_HZ + (base / 2)) / base).clamp(4, 510);
    divider as u16
}
