//! Common types used throughout the crate
//!

use core::num::NonZeroU8;
use defmt::Format;
use usb_device::{
    control::{Recipient, RequestType},
    UsbDirection,
};

/// An address that was assigned to a device by the host.
///
/// The address may or may not represent a device that is currently attached.
/// Normally device addresses are not reused, except when the address counter overflows.
///
/// This type only represents assigned addresses, and thus cannot represent the special address 0.
/// Address 0 is only used to assign an address to the device during enumeration, and should not be used by any drivers.
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

/// Represents a 16-bit binary-coded-decimal value
///
/// A 16-bit BCD represents 4 decimal digits (0-9).
#[derive(Clone, Copy, PartialEq)]
pub struct Bcd16(pub(crate) u16);

impl Bcd16 {
    /// Returns the four contained digits as separate numbers
    ///
    /// Each of the returned numbers is in the 0-9 range.
    pub fn to_digits(self) -> [u8; 4] {
        [
            ((self.0 >> 12) & 0xF) as u8,
            ((self.0 >> 8) & 0xF) as u8,
            ((self.0 >> 4) & 0xF) as u8,
            (self.0 & 0xF) as u8,
        ]
    }

    pub(crate) fn is_valid(value: u16) -> bool {
        (value >> 12 & 0xF) < 10
            && (value >> 8 & 0xF) < 10
            && (value >> 4 & 0xF) < 10
            && (value & 0xF) < 10
    }
}

impl Format for Bcd16 {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(
            fmt,
            "{}{}{}{}",
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
        defmt::write!(
            fmt,
            "{}",
            match self {
                ConnectionSpeed::Low => "low",
                ConnectionSpeed::Full => "full",
            }
        )
    }
}

/// Represents one of the four transfer types that USB supports
#[derive(Copy, Clone, PartialEq)]
#[repr(u8)]
pub enum TransferType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

/// Represents a setup packet
///
/// See [`SetupPacket::new`] for usage info.
///
/// NOTE: the fields are all public, because they must be read by the [`crate::bus::HostBus`] implementation.
///   The fields are not meant to be written to though. Use the [`SetupPacket::new`] construct instead.
pub struct SetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl SetupPacket {
    /// Construct a setup packet for for a control transfer
    ///
    /// Setup packets are then passed to the [`control_in`](crate::UsbHost::control_in) / [`control_out`](crate::UsbHost::control_out) methods.
    ///
    /// Usually this is done by drivers, not application code.
    ///
    /// The parameters make up the contents of the setup packet:
    /// - `direction`: must be set to `UsbDirection::In` when calling `control_in`, and `UsbDirection::Out` when calling `control_out`
    /// - `request_type`: if this is a `Standard`, `Class` or `Vendor` request. Note that for some standard requests there are already methods
    ///   provided on the [`crate::UsbHost`] which craft these packets for you.
    /// - `recipient`: recipient of the control request: `Device`, `Interface`, `Endpoint` or `Other` (vendor specific)
    /// - `request`, `value`: the meaning of these values depend on the `request_type`.
    ///   For standard requests these are defined by the USB specification, for class requests they are defined by the respective class specification.
    ///   For vendor requests they are (you guessed it) defined by the vendor :)
    /// - `index`: if the `recipient` is `Interface` or `Endpoint`, this field contains the respective interface or endpoint number that is being addressed.
    /// - `length`: length in bytes that will be transferred in the subsequent data stage. When calling `control_out` this must be equal to the size of the
    ///   slice that is passed in as `data`.
    ///
    pub fn new(
        direction: UsbDirection,
        request_type: RequestType,
        recipient: Recipient,
        request: u8,
        value: u16,
        index: u16,
        length: u16,
    ) -> Self {
        Self {
            request_type: (recipient as u8) | ((request_type as u8) << 5) | (direction as u8),
            request,
            value,
            index,
            length,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use usb_device::control::Request;

    #[test]
    fn test_setup_new() {
        let packet = SetupPacket::new(
            UsbDirection::In,
            RequestType::Standard,
            Recipient::Device,
            Request::GET_DESCRIPTOR,
            0x1234,
            0,
            27,
        );
        assert_eq!(packet.request_type, 0x80);
        assert_eq!(packet.request, 0x06);
        assert_eq!(packet.value, 0x1234);
        assert_eq!(packet.index, 0);
        assert_eq!(packet.length, 27);
    }

    #[test]
    fn test_bcd_digits() {
        let bcd = Bcd16(0x1234);
        assert_eq!(bcd.to_digits(), [1, 2, 3, 4]);
    }

    #[test]
    fn test_bcd_is_valid() {
        assert!(Bcd16::is_valid(0x1234));
        assert!(Bcd16::is_valid(0x9999));
        assert!(!Bcd16::is_valid(0xA000));
        assert!(!Bcd16::is_valid(0x0F09));
    }
}
