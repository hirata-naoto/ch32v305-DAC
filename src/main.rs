#![no_std]
#![no_main]

use audio::USB_PACKET_SIZE;
use ch32_hal::usbhs::{self, Driver};
use ch32_hal::usb::EndpointDataBuffer512;
use ch32_hal::{self as hal, bind_interrupts, peripherals, Config};
use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_usb::driver::{Endpoint, EndpointError, EndpointIn, EndpointOut};
use embassy_usb::Builder;
use panic_halt as _;
use static_cell::StaticCell;

mod audio;
mod i2s;

bind_interrupts!(struct Irq {
    USBHS => usbhs::InterruptHandler<peripherals::USBHS>;
    USBHS_WKUP => usbhs::WakeupInterruptHandler<peripherals::USBHS>;
});

const USB_PACKET_WORDS: usize = USB_PACKET_SIZE / 2;
const AUDIO_FIFO_CAPACITY_WORDS: usize = USB_PACKET_WORDS * 256;
const I2S_DMA_BUFFER_WORDS: usize = USB_PACKET_WORDS * 32;

static AUDIO_FIFO: Mutex<CriticalSectionRawMutex, AudioSampleFifo<AUDIO_FIFO_CAPACITY_WORDS>> =
    Mutex::new(AudioSampleFifo::new());
static AUDIO_HANDLER: StaticCell<audio::UsbAudioClass> = StaticCell::new();
static I2S_DMA_BUFFER: StaticCell<[u16; I2S_DMA_BUFFER_WORDS]> = StaticCell::new();

struct AudioSampleFifo<const N: usize> {
    data: [u16; N],
    read: usize,
    len: usize,
}

impl<const N: usize> AudioSampleFifo<N> {
    const fn new() -> Self {
        Self {
            data: [0; N],
            read: 0,
            len: 0,
        }
    }

    fn clear(&mut self) {
        self.read = 0;
        self.len = 0;
    }

    fn push_slice(&mut self, words: &[u16]) {
        if words.len() >= N {
            // FIFO 容量を超える入力は末尾の最新サンプルだけを残す。
            self.clear();
            for &word in &words[words.len() - N..] {
                self.write_word(word);
            }
            return;
        }

        let missing_space = words.len().saturating_sub(N - self.len);
        if missing_space != 0 {
            // USB 側が一時的に速かったときは古いサンプルを捨てて遅延の増えすぎを防ぐ。
            self.discard_oldest(missing_space);
        }

        for &word in words {
            self.write_word(word);
        }
    }

    fn pop_slice(&mut self, out: &mut [u16]) -> usize {
        let count = out.len().min(self.len);
        for slot in out.iter_mut().take(count) {
            *slot = self.read_word();
        }
        count
    }

    fn discard_oldest(&mut self, count: usize) {
        let discard = count.min(self.len);
        self.read = (self.read + discard) % N;
        self.len -= discard;
    }

    fn write_word(&mut self, word: u16) {
        let write = (self.read + self.len) % N;
        self.data[write] = word;
        self.len += 1;
    }

    fn read_word(&mut self) -> u16 {
        let word = self.data[self.read];
        self.read = (self.read + 1) % N;
        self.len -= 1;
        word
    }
}

fn bytes_to_words(bytes: &[u8], out: &mut [u16]) -> usize {
    let mut count = 0;
    for sample_bytes in bytes.chunks_exact(2).take(out.len()) {
        // USB から届く little-endian PCM32 を、I2S の 16-bit データレジスタへ流し込む半語列へ詰め替える。
        out[count] = u16::from_le_bytes([sample_bytes[0], sample_bytes[1]]);
        count += 1;
    }
    count
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
    // USBHS では PB7/PB6 が DP/DM になるため、ここで予約しておく。
    let _usbhs_dp = p.PB7;
    let _usbhs_dm = p.PB6;
    let dma_buffer = I2S_DMA_BUFFER.init([0; I2S_DMA_BUFFER_WORDS]);

    let mut endpoint_buffers: [EndpointDataBuffer512; 2] =
        core::array::from_fn(|_| EndpointDataBuffer512::default());
    let driver = Driver::new(p.USBHS, Irq, _usbhs_dp, _usbhs_dm, &mut endpoint_buffers);

    let mut usb_config = embassy_usb::Config::new(0x1209, 0x3050);
    usb_config.manufacturer = Some("hirata-naoto");
    usb_config.product = Some("CH32V305 USBHS Audio to I2S");
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

    let (audio_handler, mut stream_endpoint, mut feedback_endpoint) =
        audio::UsbAudioClass::new(&mut builder);
    let audio_handler = AUDIO_HANDLER.init(audio_handler);
    builder.handler(audio_handler);

    let mut usb = builder.build();
    let mut i2s = i2s::I2s2Tx::new(p.DMA1_CH5, dma_buffer);
    i2s.configure(audio::SAMPLE_RATE_HZ);
    i2s.prime(&[0; USB_PACKET_WORDS]);
    i2s.start();

    let usb_fut = usb.run();
    let receive_fut = async {
        let mut packet = [0u8; USB_PACKET_SIZE];
        let mut words = [0u16; USB_PACKET_WORDS];

        loop {
            stream_endpoint.wait_enabled().await;
            {
                // 新しいストリーム開始時は前回の残りを捨てて先頭から再生し直す。
                let mut fifo = AUDIO_FIFO.lock().await;
                fifo.clear();
            }

            loop {
                match stream_endpoint.read(&mut packet).await {
                    Ok(received) => {
                        let word_count = bytes_to_words(&packet[..received], &mut words);
                        let mut fifo = AUDIO_FIFO.lock().await;
                        // USBHS の等時転送で受けた 125 us 分の PCM を、I2S 側とは独立した FIFO へ積む。
                        fifo.push_slice(&words[..word_count]);
                    }
                    Err(EndpointError::Disabled) => {
                        let mut fifo = AUDIO_FIFO.lock().await;
                        fifo.clear();
                        break;
                    }
                    Err(EndpointError::BufferOverflow) => {}
                }
            }
        }
    };
    let playback_fut = async {
        let mut chunk = [0u16; USB_PACKET_WORDS];

        loop {
            let written = {
                let mut fifo = AUDIO_FIFO.lock().await;
                if audio::STREAM_ACTIVE.load(core::sync::atomic::Ordering::Relaxed) {
                    fifo.pop_slice(&mut chunk)
                } else {
                    fifo.clear();
                    0
                }
            };

            // FIFO が空のときは無音を補って、DMA の連続出力を途切れさせない。
            chunk[written..].fill(0);
            i2s.write_words(&chunk).await;
        }
    };
    let feedback_fut = async {
        loop {
            feedback_endpoint.wait_enabled().await;

            loop {
                // 192 kHz 固定動作なので、ホストへは毎マイクロフレーム同じ 16.16 値を返す。
                match feedback_endpoint.write(&audio::FEEDBACK_PACKET).await {
                    Ok(()) => {}
                    Err(EndpointError::Disabled) => break,
                    Err(EndpointError::BufferOverflow) => {}
                }
            }
        }
    };

    join4(usb_fut, receive_fut, playback_fut, feedback_fut).await;
    loop {}
}
