use core::sync::atomic::{AtomicBool, Ordering};

use embassy_usb::control::{InResponse, OutResponse, Recipient, Request, RequestType};
use embassy_usb::descriptor::{SynchronizationType, UsageType};
use embassy_usb::driver::{Driver, EndpointOut};
use embassy_usb::types::InterfaceNumber;
use embassy_usb::{Builder, Handler};

pub const SAMPLE_RATE_HZ: u32 = 48_000;
pub const CHANNEL_COUNT: usize = 2;
pub const BYTES_PER_SAMPLE: usize = 2;
pub const BITS_PER_SAMPLE: u8 = 16;
pub const USB_PACKET_SIZE: usize =
    (SAMPLE_RATE_HZ as usize / 1_000) * CHANNEL_COUNT * BYTES_PER_SAMPLE;

pub static STREAM_ACTIVE: AtomicBool = AtomicBool::new(false);

const USB_CLASS_AUDIO: u8 = 0x01;
const USB_SUBCLASS_AUDIO_CONTROL: u8 = 0x01;
const USB_SUBCLASS_AUDIO_STREAMING: u8 = 0x02;
const USB_PROTOCOL_IP_02_00: u8 = 0x20;

const CS_INTERFACE: u8 = 0x24;
const CS_ENDPOINT: u8 = 0x25;

const AC_HEADER: u8 = 0x01;
const AC_INPUT_TERM: u8 = 0x02;
const AC_OUTPUT_TERM: u8 = 0x03;
const AC_CLOCK_SOURCE: u8 = 0x0A;

const AS_GENERAL: u8 = 0x01;
const AS_FORMAT_TYPE: u8 = 0x02;
const EP_GENERAL: u8 = 0x01;

const CLOCK_SOURCE_ID: u8 = 0x10;
const INPUT_TERM_ID: u8 = 0x11;
const OUTPUT_TERM_ID: u8 = 0x12;

const FUNCTION_CATEGORY_DESKTOP_SPEAKER: u8 = 0x01;
const TERM_USB_STREAMING: u16 = 0x0101;
const TERM_SPEAKER: u16 = 0x0301;
const CHANNEL_CONFIG_FL_FR: u32 = 0x0000_0003;
const PCM_FORMAT_I: u32 = 0x0000_0001;

const UAC2_GET_CUR: u8 = 0x01;
const UAC2_GET_RANGE: u8 = 0x02;
const SAM_FREQ_CS: u8 = 0x01;

pub struct UsbAudioClass {
    ac_interface: InterfaceNumber,
    streaming_interface: InterfaceNumber,
}

impl UsbAudioClass {
    pub fn new<'d, D: Driver<'d>>(builder: &mut Builder<'d, D>) -> (Self, D::EndpointOut)
    where
        D::EndpointOut: EndpointOut,
    {
        let mut func = builder.function(USB_CLASS_AUDIO, 0x00, USB_PROTOCOL_IP_02_00);

        let mut ac_interface = func.interface();
        let ac_interface_number = ac_interface.interface_number();
        let mut ac_alt = ac_interface.alt_setting(
            USB_CLASS_AUDIO,
            USB_SUBCLASS_AUDIO_CONTROL,
            USB_PROTOCOL_IP_02_00,
            None,
        );

        const AC_TOTAL_LENGTH: u16 = 46;
        ac_alt.descriptor(
            CS_INTERFACE,
            &[
                AC_HEADER,
                0x00,
                0x02,
                FUNCTION_CATEGORY_DESKTOP_SPEAKER,
                (AC_TOTAL_LENGTH & 0xff) as u8,
                (AC_TOTAL_LENGTH >> 8) as u8,
                0x00,
            ],
        );
        ac_alt.descriptor(
            CS_INTERFACE,
            &[AC_CLOCK_SOURCE, CLOCK_SOURCE_ID, 0x01, 0x01, 0x00, 0x00],
        );
        ac_alt.descriptor(
            CS_INTERFACE,
            &[
                AC_INPUT_TERM,
                INPUT_TERM_ID,
                (TERM_USB_STREAMING & 0xff) as u8,
                (TERM_USB_STREAMING >> 8) as u8,
                0x00,
                CLOCK_SOURCE_ID,
                CHANNEL_COUNT as u8,
                (CHANNEL_CONFIG_FL_FR & 0xff) as u8,
                ((CHANNEL_CONFIG_FL_FR >> 8) & 0xff) as u8,
                ((CHANNEL_CONFIG_FL_FR >> 16) & 0xff) as u8,
                ((CHANNEL_CONFIG_FL_FR >> 24) & 0xff) as u8,
                0x00,
                0x00,
                0x00,
                0x00,
            ],
        );
        ac_alt.descriptor(
            CS_INTERFACE,
            &[
                AC_OUTPUT_TERM,
                OUTPUT_TERM_ID,
                (TERM_SPEAKER & 0xff) as u8,
                (TERM_SPEAKER >> 8) as u8,
                0x00,
                INPUT_TERM_ID,
                CLOCK_SOURCE_ID,
                0x00,
                0x00,
                0x00,
            ],
        );

        let mut as_interface = func.interface();
        let as_interface_number = as_interface.interface_number();
        let _ = as_interface.alt_setting(
            USB_CLASS_AUDIO,
            USB_SUBCLASS_AUDIO_STREAMING,
            USB_PROTOCOL_IP_02_00,
            None,
        );

        let mut as_alt = as_interface.alt_setting(
            USB_CLASS_AUDIO,
            USB_SUBCLASS_AUDIO_STREAMING,
            USB_PROTOCOL_IP_02_00,
            None,
        );
        as_alt.descriptor(
            CS_INTERFACE,
            &[
                AS_GENERAL,
                INPUT_TERM_ID,
                0x00,
                0x00,
                (PCM_FORMAT_I & 0xff) as u8,
                ((PCM_FORMAT_I >> 8) & 0xff) as u8,
                ((PCM_FORMAT_I >> 16) & 0xff) as u8,
                ((PCM_FORMAT_I >> 24) & 0xff) as u8,
                CHANNEL_COUNT as u8,
                (CHANNEL_CONFIG_FL_FR & 0xff) as u8,
                ((CHANNEL_CONFIG_FL_FR >> 8) & 0xff) as u8,
                ((CHANNEL_CONFIG_FL_FR >> 16) & 0xff) as u8,
                ((CHANNEL_CONFIG_FL_FR >> 24) & 0xff) as u8,
                0x00,
            ],
        );
        as_alt.descriptor(
            CS_INTERFACE,
            &[
                AS_FORMAT_TYPE,
                0x01,
                BYTES_PER_SAMPLE as u8,
                BITS_PER_SAMPLE,
            ],
        );

        let stream_endpoint = as_alt.endpoint_isochronous_out(
            None,
            USB_PACKET_SIZE as u16,
            1,
            SynchronizationType::Adaptive,
            UsageType::DataEndpoint,
            &[],
        );
        as_alt.descriptor(CS_ENDPOINT, &[EP_GENERAL, 0x00, 0x00, 0x00, 0x00, 0x00]);

        (
            Self {
                ac_interface: ac_interface_number,
                streaming_interface: as_interface_number,
            },
            stream_endpoint,
        )
    }
}

impl Handler for UsbAudioClass {
    fn control_out(&mut self, _req: Request, _buf: &[u8]) -> Option<OutResponse> {
        None
    }

    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        if req.request_type != RequestType::Class || req.recipient != Recipient::Interface {
            return None;
        }

        if (req.index as u8) != u8::from(self.ac_interface) {
            return None;
        }

        if ((req.index >> 8) as u8) != CLOCK_SOURCE_ID || ((req.value >> 8) as u8) != SAM_FREQ_CS {
            return None;
        }

        match req.request {
            UAC2_GET_CUR => {
                let bytes = SAMPLE_RATE_HZ.to_le_bytes();
                buf[..bytes.len()].copy_from_slice(&bytes);
                Some(InResponse::Accepted(&buf[..bytes.len()]))
            }
            UAC2_GET_RANGE => {
                let response = {
                    let mut bytes = [0u8; 14];
                    bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
                    bytes[2..6].copy_from_slice(&SAMPLE_RATE_HZ.to_le_bytes());
                    bytes[6..10].copy_from_slice(&SAMPLE_RATE_HZ.to_le_bytes());
                    bytes[10..14].copy_from_slice(&0u32.to_le_bytes());
                    bytes
                };
                buf[..response.len()].copy_from_slice(&response);
                Some(InResponse::Accepted(&buf[..response.len()]))
            }
            _ => None,
        }
    }

    fn set_alternate_setting(&mut self, iface: InterfaceNumber, alternate: u8) {
        if iface == self.streaming_interface {
            STREAM_ACTIVE.store(alternate == 1, Ordering::Relaxed);
        }
    }

    fn reset(&mut self) {
        STREAM_ACTIVE.store(false, Ordering::Relaxed);
    }
}
