use heapless::Vec;
use usb_device::class_prelude::UsbBus;
use usb_device::UsbError;
use usbd_hid::hid_class::{ReportInfo, ReportType};
use usbd_hid::Result as UsbResult;
use usbd_hid::{descriptor::generator_prelude::*, hid_class::HIDClass};

use crate::{OIError, OiReport};

use super::OpenInputHidReport;

#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = GENERIC_DESKTOP, usage = KEYBOARD, report_id = 0x02) = {
        (usage_page = KEYBOARD, usage_min = 0xE0, usage_max = 0xE7) = {
            #[packed_bits 8] #[item_settings data,variable,absolute] modifier=input;
        };
        (usage_min = 0x00, usage_max = 0xFF) = {
            #[item_settings constant,variable,absolute] reserved=input;
        };
        (usage_page = LEDS, usage_min = 0x01, usage_max = 0x05) = {
            #[packed_bits 5] #[item_settings data,variable,absolute] leds=output;
        };
        (usage_page = KEYBOARD, usage_min = 0x00, usage_max = 0xDD) = {
            #[item_settings data,array,absolute] keycodes=input;
        };
    },
    (collection = APPLICATION, usage_page = VENDOR_DEFINED_START, usage = 0x00) = {
        (report_id = 0x20,) = {
            (usage = 0x00,) = {
                #[item_settings data,array,absolute] input_short_buf=input;
            };
            (usage = 0x00,) = {
                #[item_settings data,array,absolute] out_short_buf=output;
            };
        }
    },
    (collection = APPLICATION, usage_page = VENDOR_DEFINED_START, usage = 0x00) = {
        (report_id = 0x21,) = {
            (usage = 0x00,) = {
                #[item_settings data,array,absolute] input_long_buf=input;
            };
            (usage = 0x00,) = {
                #[item_settings data,array,absolute] out_long_buf=output;
            };
        }
    }
)]
#[derive(Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct OiKeyboardReport {
    pub modifier: u8,
    pub reserved: u8,
    pub leds: u8,
    pub keycodes: [u8; 6],
    // openinput
    input_short_buf: [u8; 8],
    out_short_buf: [u8; 8],

    input_long_buf: [u8; 32],
    out_long_buf: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyboardReportId {
    OpenInputShort = 0x20,
    OpenInputLong = 0x21,
    Keyboard = 0x02,
}
impl TryFrom<u8> for KeyboardReportId {
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x02 => Ok(KeyboardReportId::Keyboard),
            0x20 => Ok(KeyboardReportId::OpenInputShort),
            0x21 => Ok(KeyboardReportId::OpenInputLong),
            _ => Err(()),
        }
    }
}

// TODO use serialize/deserialize
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum OiKeyboardOutputReport<'a> {
    /// Keyboard leds bitfeild
    Keyboard(u8),
    /// Openinput short/long report
    OpenInput(OiReport<'a>),
}

#[derive(Debug, Clone, Default, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct KeyboardInputReport {
    pub modifier: u8,
    pub reserved: u8,
    pub keycodes: [u8; 6],
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum OiKeyboardInputReport<'a> {
    /// Keyboard report
    Keyboard(KeyboardInputReport),
    /// Openinput short/long report
    OpenInput(OiReport<'a>),
}

impl<'a> Serialize for OiKeyboardInputReport<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            OiKeyboardInputReport::Keyboard(kb) => {
                // report id + 8 bytes
                let mut s = serializer.serialize_tuple(8)?;
                // s.serialize_element(&(KeyboardReportId::Keyboard as u8))?;
                s.serialize_element(&kb.modifier)?;
                s.serialize_element(&kb.reserved)?;
                s.serialize_element(&kb.keycodes)?;
                s.end()
            }
            OiKeyboardInputReport::OpenInput(oi) => oi.serialize(serializer),
        }
    }
}

impl OpenInputHidReport for OiKeyboardReport {
    type PullReport<'a> = OiKeyboardOutputReport<'a>;
    type PushReport<'a> = OiKeyboardInputReport<'a>;
    type ReportId = KeyboardReportId;

    fn pull_ep_out<'a, 'ep, B: UsbBus>(
        &'a mut self,
        hid: &mut HIDClass<'ep, B>,
    ) -> Result<Self::PullReport<'a>, OIError> {
        let mut temp_buf = [0; super::REPORT_BUFFER_SIZE];
        // TODO should probably read from interrupt out ep as well (as per spec)
        let report = hid.pull_raw_report(&mut temp_buf)?;
        let ReportInfo {
            len,
            report_id,
            report_type,
        } = report;

        // TODO what does pull_raw_report actually return, will return either output or feature or does it only return one?
        match report_type {
            ReportType::Output | ReportType::Feature => (),
            // pulling report should _only_ give output or feature reports
            ReportType::Input | ReportType::Reserved => {
                return Err(usb_device::UsbError::InvalidState.into())
            }
        }

        let buf = &temp_buf[..len];

        match Self::ReportId::try_from(report_id).map_err(|_| UsbError::ParseError)? {
            KeyboardReportId::Keyboard => {
                if buf.len() == 1 {
                    Ok(OiKeyboardOutputReport::Keyboard(buf[0]))
                } else {
                    Err(OIError::FuckyBuffer)
                }
            }
            KeyboardReportId::OpenInputShort => {
                if buf.len() == 8 {
                    // TODO: do i really need to re-zero here?
                    self.out_short_buf = [0; 8];
                    self.out_short_buf.copy_from_slice(buf);
                    Ok(OiKeyboardOutputReport::OpenInput(
                        OiReport::read(&self.input_short_buf).map_err(|_| UsbError::ParseError)?,
                    ))
                } else {
                    Err(OIError::FuckyBuffer)
                }
            }
            KeyboardReportId::OpenInputLong => {
                if buf.len() == 32 {
                    // TODO: do i really need to re-zero here?
                    self.out_long_buf = [0; 32];
                    self.out_long_buf.copy_from_slice(buf);
                    Ok(OiKeyboardOutputReport::OpenInput(
                        OiReport::read(&self.input_long_buf).map_err(|_| UsbError::ParseError)?,
                    ))
                } else {
                    Err(OIError::FuckyBuffer)
                }
            }
        }
    }

    fn push_report<'b, 'ep, B: UsbBus>(
        &mut self,
        hid: &mut HIDClass<'ep, B>,
        report: Self::PushReport<'b>,
    ) -> Result<(), OIError> {
        let mut buf = [0; 64];
        let m = ssmarshal::serialize(&mut buf, &report).map_err(|_| OIError::SerializationError)?;
        hid.push_raw_input(&buf[..m])?;
        Ok(())
    }
}

// pub fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
//     if self.expect_interrupt_in_complete {
//         return Ok(0);
//     }

//     if data.len() >= 8 {
//         self.expect_interrupt_in_complete = true;
//     }

//     match self.endpoint_interrupt_in.write(data) {
//         Ok(count) => Ok(count),
//         Err(UsbError::WouldBlock) => Ok(0),
//         Err(_) => Err(Error),
//     }
// }
