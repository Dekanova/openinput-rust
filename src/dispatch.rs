use heapless::{FnvIndexMap, String, Vec};

use crate::{OiReport, OPENINPUT_SHORT_REPORT_ID};

const ERROR_FUNCTION_PAGE: u8 = 0xFF;
const INFO_FUNCTION_PAGE: u8 = 0x00;

// fn params may be 6 or 29 bytes

// TODO UnsupportedFunction should return what requested page and ID caused the error
/// https://openinput.readthedocs.io/projects/protocol/en/latest/device-protocol/functions/ff_error.html
#[derive(Debug)]
pub enum Error {
    InvalidValue(u8),
    UnsupportedFunction,
    Custom([u8; LONG_LEN - ERROR_PREFIX_LEN]),
}

impl Error {
    pub fn id(&self) -> u8 {
        match self {
            Self::InvalidValue(_) => 0x01,
            Self::UnsupportedFunction => 0x02,
            Self::Custom(_) => 0xFE,
        }
    }

    pub(crate) fn serialize_error(&self, page: u8, id: u8) -> Vec<u8, 32> {
        let invalid_data = &mut [page, id, 0];
        let unsupported_data = &[page, id];
        let o = match *self {
            Error::InvalidValue(index) => {
                invalid_data[2] = index;
                OiReport {
                    id: OPENINPUT_SHORT_REPORT_ID,
                    function_page: ERROR_FUNCTION_PAGE,
                    function_id: self.id(),
                    data: invalid_data,
                }
            }
            Error::UnsupportedFunction => OiReport {
                id: OPENINPUT_SHORT_REPORT_ID,
                function_page: ERROR_FUNCTION_PAGE,
                function_id: self.id(),
                data: unsupported_data,
            },
            // only custom error type might need to fit in a long report
            Error::Custom(ascii) => {
                let mut rep = OiReport {
                    id: OPENINPUT_SHORT_REPORT_ID,
                    function_page: ERROR_FUNCTION_PAGE,
                    function_id: self.id(),
                    data: &[],
                };
                // bit too large but whatever
                let mut buf = [0; LONG_LEN];

                let mut nulls = ascii.iter().enumerate().filter(|(_, &char)| char == 0);
                // if first null ascii char is at index 3 it can fit in a short report, otherwise long report
                let len = match nulls.next() {
                    Some((i, _)) => {
                        if i <= 3 {
                            rep.id = super::OPENINPUT_SHORT_REPORT_ID;
                            i
                        } else {
                            i
                        }
                    }
                    None => {
                        rep.id = super::OPENINPUT_SHORT_REPORT_ID;
                        // no null means full report
                        LONG_LEN - ERROR_PREFIX_LEN
                    }
                };
                buf[..len].copy_from_slice(&ascii[..len]);

                rep
            }
        };

        o.into()
    }
}

/// ReportId, FnPage, FnId
const DISPATCH_PREFIX_LEN: usize = 3;
/// ReportId, FnPage (0xFF), ErrorId, FnPage, FnId
const ERROR_PREFIX_LEN: usize = DISPATCH_PREFIX_LEN + 2;

const SHORT_LEN: usize = 8;
const LONG_LEN: usize = 32;

// TODO better names
const DISPATCH_LONG_RET_LEN: usize = LONG_LEN - DISPATCH_PREFIX_LEN;
const DISPATCH_SHORT_RET_LEN: usize = SHORT_LEN - DISPATCH_PREFIX_LEN;

/// newtype to enforce proper output serailization
pub struct DispatchResponse(Vec<u8, DISPATCH_LONG_RET_LEN>);

impl DispatchResponse {
    // TODO dont panic
    /// pad response to fill into report size
    fn report<'a>(&'a mut self, page: u8, fn_id: u8) -> OiReport<'a> {
        if self.0.len() > DISPATCH_SHORT_RET_LEN {
            self.0.resize(DISPATCH_SHORT_RET_LEN, 0).unwrap();
            OiReport::new_short(page, fn_id, self.0.as_slice().try_into().unwrap())
        } else {
            self.0.resize(DISPATCH_LONG_RET_LEN, 0).unwrap();
            OiReport::new_long(page, fn_id, self.0.as_slice().try_into().unwrap())
        }
    }
}

impl From<Vec<u8, DISPATCH_LONG_RET_LEN>> for DispatchResponse {
    fn from(src: Vec<u8, DISPATCH_LONG_RET_LEN>) -> Self {
        Self(src)
    }
}

type DispatchReturn = Result<DispatchResponse, Error>;
type DispatchFn = for<'input, 'ctx> fn(&[u8], DispatchContext<'ctx>) -> DispatchReturn;

// NOTE: table lookups are O(2) but they need to do hashing before lookup so O(n) without hashing would probably be faster.
type DispatchTable = FnvIndexMap<u8, FnvIndexMap<u8, DispatchFn, 8>, 8>;

pub struct DispatchContext<'a> {
    table: &'a DispatchTable,
    meta: &'a DispatchMeta,
}

pub struct Dispatch {
    /// 8 pages, max 8 functions per page (implementation detail)
    table: DispatchTable,
    pub meta: DispatchMeta,
}

/// https://openinput.readthedocs.io/projects/protocol/en/latest/device-protocol/functions/00_info.html
pub struct DispatchMeta {
    protocol_version: [u8; 3],
    firmware_vendor: Vec<u8, DISPATCH_LONG_RET_LEN>,
    firmware_version: Vec<u8, DISPATCH_LONG_RET_LEN>,
    device_name: Vec<u8, DISPATCH_LONG_RET_LEN>,
}

impl Dispatch {
    // panics if !(5 <= `data.len()` <= 29)
    pub fn dispatch_raw(&self, page: u8, id: u8, data: &[u8]) -> DispatchReturn {
        assert!(data.len() >= DISPATCH_SHORT_RET_LEN && data.len() <= DISPATCH_LONG_RET_LEN);
        let func = match self.table.get(&page).and_then(|fn_page| fn_page.get(&id)) {
            Some(func) => func,
            None => return Err(Error::UnsupportedFunction),
        };

        let ctx = DispatchContext {
            table: &self.table,
            meta: &self.meta,
        };
        func(data, ctx)
    }

    /// construct from raw function table, this will not implement functions required to be compliant with openinput's spec
    pub const fn new_raw(table: DispatchTable, meta: DispatchMeta) -> Self {
        Self { table, meta }
    }
}

impl Default for Dispatch {
    fn default() -> Self {
        let mut table = FnvIndexMap::<u8, FnvIndexMap<u8, DispatchFn, 8>, 8>::new();

        let mut info_page = FnvIndexMap::<u8, DispatchFn, 8>::new();

        info_page
            .insert(info_table::INFO_VERSION, info_table::protocol_version)
            .ok()
            .expect("failed to insert version function into dispatch table");
        info_page
            .insert(info_table::INFO_FIRMWARE_INFO, info_table::firmware_info)
            .ok()
            .expect("failed to insert firmware_info function into dispatch table");
        info_page
            .insert(
                info_table::INFO_SUPPORTED_FUNCTION_PAGES,
                info_table::supported_fn_pages,
            )
            .ok()
            .expect("failed to insert supported_fn_pages function into dispatch table");
        info_page
            .insert(
                info_table::INFO_SUPPORTED_FUNCTIONS,
                info_table::supported_fns,
            )
            .ok()
            .expect("failed to insert supported_fns function into dispatch table");

        match table.insert(INFO_FUNCTION_PAGE, info_page) {
            Ok(_) => (),
            Err(_) => panic!("failed to insert info page into dispatch table"),
        }

        let meta = DispatchMeta {
            firmware_vendor: Vec::from_slice(b"Unspecified Vendor").unwrap(),
            firmware_version: Vec::from_slice(b"Unspecified Version").unwrap(),
            protocol_version: super::PROTOCOL_VERSION,
            device_name: Vec::from_slice(b"Unspecified Name").unwrap(),
        };

        Self::new_raw(table, meta)
    }
}

mod info_table {
    use super::*;

    pub const INFO_VERSION: u8 = 0x00;
    pub const INFO_FIRMWARE_INFO: u8 = 0x01;
    pub const INFO_SUPPORTED_FUNCTION_PAGES: u8 = 0x02;
    pub const INFO_SUPPORTED_FUNCTIONS: u8 = 0x03;

    pub fn protocol_version(_: &[u8], ctx: DispatchContext) -> DispatchReturn {
        Ok(Vec::from_slice(&ctx.meta.protocol_version).unwrap().into())
    }

    pub enum FirmwareInfoParam {
        Vendor = 0,
        Version = 1,
        DeviceName = 2,
    }

    impl TryFrom<u8> for FirmwareInfoParam {
        type Error = Error;

        fn try_from(value: u8) -> Result<Self, Self::Error> {
            match value {
                0 => Ok(Self::Vendor),
                1 => Ok(Self::Version),
                2 => Ok(Self::DeviceName),
                _ => Err(Error::InvalidValue(0)),
            }
        }
    }

    pub fn firmware_info(input: &[u8], ctx: DispatchContext) -> DispatchReturn {
        let info: FirmwareInfoParam = input[0].try_into()?;
        Ok(match info {
            FirmwareInfoParam::Vendor => &ctx.meta.firmware_vendor,
            FirmwareInfoParam::Version => &ctx.meta.firmware_version,
            FirmwareInfoParam::DeviceName => &ctx.meta.device_name,
        }
        .clone()
        .into())
    }

    pub fn supported_fn_pages(input: &[u8], ctx: DispatchContext) -> DispatchReturn {
        let start = input[0] as usize;

        let mut pages: Vec<u8, 8> = Vec::from_iter(ctx.table.iter().map(|(&k, _)| k));
        pages.sort_unstable();
        // NOTE: implementation limits to 8 pages, if we use a long report we don't need to worry about partial sets
        let element_list = pages.get(start..).ok_or(Error::InvalidValue(0))?;

        let mut output = Vec::new();
        output
            .extend_from_slice(&[element_list.len() as u8, 0])
            .unwrap();
        output.extend_from_slice(&element_list).unwrap();

        Ok(output.into())
    }

    pub fn supported_fns(input: &[u8], ctx: DispatchContext) -> DispatchReturn {
        let page = input[0];
        let start = input[1] as usize;

        // TODO is this error invalid input or unsupported function?
        let page = ctx.table.get(&page).ok_or(Error::UnsupportedFunction)?;
        let mut functions: Vec<u8, 8> = Vec::from_iter(page.iter().map(|(&k, _)| k));
        functions.sort_unstable();
        // NOTE: implementation limits 8 functions/page, if we use a long report we don't need to worry about partial sets
        let element_list = functions.get(start..).ok_or(Error::InvalidValue(0))?;

        let mut output = Vec::new();
        output
            .extend_from_slice(&[element_list.len() as u8, 0])
            .unwrap();
        output.extend_from_slice(&element_list).unwrap();

        Ok(output.into())
    }
}
