pub use keyboard::OiKeyboardReport;
use usb_device::class_prelude::UsbBus;
use usb_device::UsbError;
use usbd_hid::hid_class::HIDClass;

mod dispatch;
#[cfg(feature = "dispatch")]
pub mod keyboard;

// TODO i'd like to add a new page for dispatch with params split into parts for larger requests/replies (refrence descriptor is 32 bytes but we can be 64 for USB FS)
// TODO I would like to have OiHidClass have a type param for each descriptor so I can use it internally, but that may mess with ppl who want to realloc the class
// TODO Are supported functions/pages required to be in a specific order? I've sorted the response for supported fn/pages since underlying structure iterates by order of insertion
// TODO supported functions/pages should return the device relative set
// TODO AUTH PLEASE FOR THE LOVE OF GOD

const OPENINPUT_MAX_REPORT_SIZE: usize = 32;
// TODO would like to not have this, reports shouldn't be larger than 64 bytes, though this is different for usb 2.0 HS (max 1024 bytes)
// max size of OpenInput is 32 and max of keyboard (currently the only class) is 5 bits (or just 1 byte)
const REPORT_BUFFER_SIZE: usize = 64;

const OPENINPUT_SHORT_REPORT_ID: u8 = 0x20;
const OPENINPUT_LONG_REPORT_ID: u8 = 0x21;

/// OpenInput Progocol version [major, minor, patch]
pub const PROTOCOL_VERSION: [u8; 3] = [0, 0, 1];

pub struct OpenInputHIDClass<'ep, B: UsbBus, Report: OpenInputHidReport> {
    pub inner: HIDClass<'ep, B>,
    // inner report
    pub report: Report,
}

impl<'ep, B: UsbBus, R: OpenInputHidReport> OpenInputHIDClass<'ep, B, R> {
    pub fn new(hid: HIDClass<'ep, B>) -> Self {
        Self {
            inner: hid,
            report: R::default(),
        }
    }

    pub fn pull_host_data<'a>(&'a mut self) -> Result<R::PullReport<'a>, OpenInputReportError> {
        let Self { inner, report } = self;
        report.pull_ep_out(inner)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OiReportId {
    Short = OPENINPUT_SHORT_REPORT_ID,
    Long = OPENINPUT_LONG_REPORT_ID,
    Keyboard = 0x02,
}

impl OiReportId {
    pub fn id(&self) -> u8 {
        *self as u8
    }
}

impl TryFrom<u8> for OiReportId {
    type Error = ();
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x20 => Ok(OiReportId::Short),
            0x21 => Ok(OiReportId::Long),
            _ => Err(()),
        }
    }
}

// TODO go back thru and fix errors
pub enum OpenInputReportError {
    InternalError,
    FuckyBuffer,
    UsbError(UsbError),
}

impl From<UsbError> for OpenInputReportError {
    fn from(src: UsbError) -> Self {
        OpenInputReportError::UsbError(src)
    }
}

pub trait OpenInputHidReport: Default {
    // TODO maybe just from?
    type ReportId: TryFrom<u8>;
    type PullReport<'a>
    where
        Self: 'a;
    type PushReport<'a>
    where
        Self: 'a;

    // TODO I am sorta abusing UsbError for my own errors, should probably use a custom err type
    fn pull_ep_out<'a, 'ep, B: UsbBus>(
        &'a mut self,
        hid: &mut HIDClass<'ep, B>,
    ) -> Result<Self::PullReport<'a>, OpenInputReportError>;

    fn push_report<'b, 'ep, B: UsbBus>(
        &mut self,
        hid: &mut HIDClass<'ep, B>,
        report: Self::PushReport<'b>,
    ) -> Result<(), OpenInputReportError>;
}

pub struct OiReport<'a> {
    id: u8,
    function_page: u8,
    function_id: u8,
    data: &'a [u8],
}

impl<'a> OiReport<'a> {
    pub const fn read(bytes: &'a [u8]) -> Result<Self, ()> {
        if bytes.len() != 8 || bytes.len() != 32 {
            return Err(());
        }
        let (id, function_page, function_id, data) = if let [id, page, fn_id, data @ ..] = bytes {
            (*id, *page, *fn_id, data)
        } else {
            return Err(());
        };
        Ok(OiReport {
            id,
            function_page,
            function_id,
            data,
        })
    }

    // TODO use consts for len
    pub const fn new_short(page: u8, fn_id: u8, data: &'a [u8; 5]) -> Self {
        OiReport {
            id: OPENINPUT_SHORT_REPORT_ID,
            function_page: page,
            function_id: fn_id,
            data,
        }
    }

    pub const fn new_long(page: u8, fn_id: u8, data: &'a [u8; 29]) -> Self {
        OiReport {
            id: OPENINPUT_LONG_REPORT_ID,
            function_page: page,
            function_id: fn_id,
            data,
        }
    }
}

impl<'a> From<OiReport<'a>> for heapless::Vec<u8, 32> {
    fn from(src: OiReport<'a>) -> Self {
        let mut v = heapless::Vec::new();
        v.extend_from_slice(&[src.id, src.function_page, src.function_id])
            .unwrap();
        v.extend_from_slice(src.data).unwrap();
        v
    }
}

#[cfg(test)]
mod tests {
    use usbd_hid::descriptor::SerializedDescriptor;

    use super::*;

    #[test]
    fn a() {
        OiKeyboardReport::default();
    }

    #[test]
    /// make sure generated descriptor roughly equals openinput's
    fn conformance() {
        let desc = OiKeyboardReport::desc();
        let desc_hex = hex::encode(desc);
        let oi = hex::encode(OI_DESC);

        println!("got\nexpect\n{}\n{}", desc_hex, oi);
        assert!(desc_hex.contains(&oi), "\n{:x?}\n{:x?}", desc, OI_DESC);
    }

    // TODO discuss ordering and derived value diff with openinput ppl
    // modified from https://github.com/openinput-fw/openinput/blob/a8723282bd50aa01a2062d9289c16087c4712c7e/src/protocol/reports.h
    const OI_DESC: &[u8] = &[
        /* clang-format off */
        /* short report */
        0x06, 0x00, 0xff, /* USAGE_PAGE (Vendor Page) */
        0x09, 0x00, /* USAGE (Vendor Usage 0) */
        0xa1, 0x01, /* COLLECTION (Application) */
        0x85, 0x20, /*  REPORT_ID (0x20) */
        0x09, 0x00, /*  USAGE (Vendor Usage 0) */
        /* derived from keyboard */
        // 0x15, 0x00,			/*  LOGICAL MINIMUM (0) */
        // 0x26, 0xff, 0x00,		/*  LOGICAL MAXIMUM (255) */
        // 0x75, 0x08,			/*  REPORT_SIZE (8) */
        0x95, 0x08, /*  REPORT_COUNT (8) */
        0x81, 0x00, /*  INPUT (Data,Arr,Abs) */
        0x09, 0x00, /*  USAGE (Vendor Usage 0) */
        0x91, 0x00, /*  OUTPUT (Data,Arr,Abs) */
        0xc0, /* END_COLLECTION */
        /* long report */
        0x06, 0x00, 0xff, /* USAGE_PAGE (Vendor Page) */
        0x09, 0x00, /* USAGE (Vendor Usage 0) */
        0xa1, 0x01, /* COLLECTION (Application) */
        0x85, 0x21, /*  REPORT_ID (0x21) */
        0x09, 0x00, /*  USAGE (Vendor Usage 0) */
        /* derived from above */
        // 0x15, 0x00,			/*  LOGICAL MINIMUM (0) */
        // 0x26, 0xff, 0x00,		/*  LOGICAL MAXIMUM (255) */
        // 0x75, 0x08,			/*  REPORT_SIZE (8) */
        0x95, 0x20, /*  REPORT_COUNT (32) */
        0x81, 0x00, /*  INPUT (Data,Arr,Abs) */
        0x09, 0x00, /*  USAGE (Vendor Usage 0) */
        0x91, 0x00, /*  OUTPUT (Data,Arr,Abs) */
        0xc0, /* END_COLLECTION */
              /* clang-format on */
    ];
}
