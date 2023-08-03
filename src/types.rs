use core::num::NonZeroU8;
use defmt::Format;
use usb_device::{UsbDirection, control::{Recipient, RequestType}};

/// An address that was assigned to a device by the host.
///
/// The address may or may not represent a device that is currently attached.
/// Normally device addresses are not reused, except when the address counter overflows.
///
/// This type only represents assigned addresses, and thus cannot represent the special address 0.
/// Address 0 is only used to assign an address to the device, and should not be used by any drivers.
#[derive(Clone, Copy, PartialEq, Format)]
pub struct DeviceAddress(pub(crate) NonZeroU8);

impl From<DeviceAddress> for u16 {
    fn from(value: DeviceAddress) -> Self {
        u8::from(value.0) as u16
    }
}

impl From<DeviceAddress> for u8 {
    fn from(value: DeviceAddress) -> Self {
        u8::from(value.0)
    }
}

/// Refers to a physical port, where a device can be attached.
#[derive(Clone, Copy, PartialEq, Format)]
pub struct Port(u8);

impl Port {
    pub const ZERO: Port = Port(0);
}

/// VendorId and ProductId from a device descriptor
#[derive(Clone, Copy, PartialEq, Format)]
pub struct VidPid(u16, u16);

#[derive(Clone, Copy, PartialEq)]
pub struct Bcd16(pub(crate) u16);

impl Format for Bcd16 {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(
            fmt, "{}{}{}{}",
            (self.0 >> 12) & 0xF,
            (self.0 >> 8) & 0xF,
            (self.0 >> 4) & 0xF,
            self.0 & 0xF,
        )
    }
}

/// Refers to the speed at which a device operates
#[derive(Copy, Clone, PartialEq)]
pub enum ConnectionSpeed {
    /// USB 1.0 low speed
    Low,
    /// USB 1.0 full speed
    Full,
}

impl Format for ConnectionSpeed {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "{}", match self {
            ConnectionSpeed::Low => "low",
            ConnectionSpeed::Full => "full",
        })
    }
}

#[derive(Copy, Clone)]
#[repr(u8)]
pub enum TransferType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

#[repr(u8)]
pub enum DescriptorType {
    Device = 1,
    Configuration = 2,
    String = 3,
    Interface = 4,
    Endpoint = 5,
}

pub struct SetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl SetupPacket {
    pub fn new(direction: UsbDirection, request_type: RequestType, recipient: Recipient, request: u8, value: u16, index: u16, length: u16) -> Self {
        Self {
            request_type: (recipient as u8) | ((request_type as u8) << 5) | (direction as u8),
            request,
            value,
            index,
            length,
        }
    }
}
