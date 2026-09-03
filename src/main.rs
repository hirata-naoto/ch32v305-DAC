#![no_std]
#![no_main]

use audio::USB_PACKET_SIZE;
use ch32_hal::otg_fs::{self, Driver};
use ch32_hal::usb::EndpointDataBuffer512;
use ch32_hal::{self as hal, bind_interrupts, peripherals, Config};
use embassy_executor::Spawner;
use embassy_futures::join::join3;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_usb::driver::{Endpoint, EndpointError, EndpointOut};
use embassy_usb::Builder;
use panic_halt as _;
use static_cell::StaticCell;

mod audio;
mod i2s;

bind_interrupts!(struct Irq {
    OTG_FS => otg_fs::InterruptHandler<peripherals::OTG_FS>;
});

static AUDIO_QUEUE: Channel<CriticalSectionRawMutex, UsbPacket, 8> = Channel::new();
static AUDIO_HANDLER: StaticCell<audio::UsbAudioClass> = StaticCell::new();

#[derive(Clone, Copy)]
struct UsbPacket {
    len: usize,
    data: [u8; USB_PACKET_SIZE],
}

impl Default for UsbPacket {
    fn default() -> Self {
        Self {
            len: 0,
            data: [0; USB_PACKET_SIZE],
        }
    }
}

#[embassy_executor::main(entry = "qingke_rt::entry")]
async fn main(_spawner: Spawner) -> ! {
    let config = Config {
        // USB/I2S のタイミング計算を 144 MHz 前提でそろえる。
        rcc: hal::rcc::Config::SYSCLK_FREQ_144MHZ_HSI,
        ..Default::default()
    };
    let p = hal::init(config);

    // SPI2/I2S2 に割り当てるピンを確保して他用途への再利用を防ぐ。
    let _spi2 = p.SPI2;
    let _i2s_ws = p.PB12;
    let _i2s_ck = p.PB13;
    let _i2s_sd = p.PB15;

    let mut endpoint_buffers: [EndpointDataBuffer512; 1] =
        core::array::from_fn(|_| EndpointDataBuffer512::default());
    let driver = Driver::new(p.OTG_FS, p.PA12, p.PA11, &mut endpoint_buffers);

    let mut usb_config = embassy_usb::Config::new(0x1209, 0x3050);
    usb_config.manufacturer = Some("hirata-naoto");
    usb_config.product = Some("CH32V305 USB Audio to I2S");
    usb_config.serial_number = Some("0001");
    usb_config.device_class = 0x00;
    usb_config.device_sub_class = 0x00;
    usb_config.device_protocol = 0x00;
    usb_config.composite_with_iads = false;
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = 64;

    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 64];
    let mut msos_descriptor = [0; 64];
    let mut control_buf = [0; 64];

    // USB Audio Class の各種記述子とエンドポイントを組み立てる。
    let mut builder = Builder::new(
        driver,
        usb_config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    let (audio_handler, mut stream_endpoint) = audio::UsbAudioClass::new(&mut builder);
    let audio_handler = AUDIO_HANDLER.init(audio_handler);
    builder.handler(audio_handler);

    let mut usb = builder.build();
    let mut i2s = i2s::I2s2Tx::new();
    i2s.configure(audio::SAMPLE_RATE_HZ);

    let usb_fut = usb.run();
    let receive_fut = async {
        loop {
            stream_endpoint.wait_enabled().await;

            loop {
                let mut packet = UsbPacket::default();
                match stream_endpoint.read(&mut packet.data).await {
                    Ok(received) => {
                        // USB 等時転送で受けた 1 ms 分の PCM を再生キューへ渡す。
                        packet.len = received;
                        AUDIO_QUEUE.send(packet).await;
                    }
                    Err(EndpointError::Disabled) => break,
                    Err(EndpointError::BufferOverflow) => {}
                }
            }
        }
    };
    let playback_fut = async {
        loop {
            let packet = AUDIO_QUEUE.receive().await;
            // 受信順を保ったまま I2S 送信へ流し込む。
            i2s.write_packet(&packet.data[..packet.len]).await;
        }
    };

    join3(usb_fut, receive_fut, playback_fut).await;
    loop {}
}
