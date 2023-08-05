//! Types for (standard) descriptors
//!
//! This module contains types to represent various USB descriptors.
//!
//! The [`parse`] submodule contains functions for parsing raw descriptors into these structures.
//!
//! All descriptors have a common framing: the first two bytes contain the descriptor **length** and **type** respectively.
//! This framing is represented by the [`Descriptor`] type.
//!
//! To turn raw descriptor data into a [`Descriptor`] use the [`parse::any_descriptor`] function.
//!
//! Such a descriptor can then be interpreted further, by examining the [`Descriptor::descriptor_type`]:
//! - If the type matches one of the 5 standard types ([`TYPE_DEVICE`], [`TYPE_CONFIGURATION`], [`TYPE_STRING`], [`TYPE_INTERFACE`], [`TYPE_ENDPOINT`]),
//!   then it's `data` can further be parsed by the respective methods in the [`parse`] module.
//! - Otherwise it's up to the driver to interpret the descriptor.
//!

use crate::types::{Bcd16, TransferType};
use usb_device::UsbDirection;
use defmt::Format;

/// [`descriptor_type`](Descriptor::descriptor_type) identifying a [`DeviceDescriptor`]
pub const TYPE_DEVICE: u8 = 1;
/// [`descriptor_type`](Descriptor::descriptor_type) identifying a [`ConfigurationDescriptor`]
pub const TYPE_CONFIGURATION: u8 = 2;
/// [`descriptor_type`](Descriptor::descriptor_type) identifying a `StringDescriptor` (not yet implemented)
pub const TYPE_STRING: u8 = 3;
/// [`descriptor_type`](Descriptor::descriptor_type) identifying an [`InterfaceDescriptor`]
pub const TYPE_INTERFACE: u8 = 4;
/// [`descriptor_type`](Descriptor::descriptor_type) identifying an [`EndpointDescriptor`]
pub const TYPE_ENDPOINT: u8 = 5;

/// Outer framing of a descriptor
pub struct Descriptor<'a> {
    /// Total length of the descriptor, including this length byte itself and the `descriptor_type` byte
    pub length: u8,
    /// Type of descriptor. If this is a standard descriptor, it corresponds to one of the `TYPE_*` constants,
    /// otherwise it is class or vendor specific.
    pub descriptor_type: u8,
    /// Remaining data of the descriptor. Usually `length - 2` bytes long, except the descriptor may be truncated
    /// if less data was requested, or the data did not fully fit into the control buffer.
    pub data: &'a [u8],
}

/// A device descriptor describes general information about a USB device. It includes information that applies
/// globally to the device and all of the device’s configurations. A USB device has only one device descriptor.
#[derive(Format)]
pub struct DeviceDescriptor {
    /// USB Specification Release Number in Binary-Coded Decimal (i.e., 2.10 is 210H).
    ///
    /// This field identifies the release of the USB Specification with which the device and its descriptors are compliant.
    pub usb_release: Bcd16,
    /// Class code (assigned by the USB-IF).
    ///
    /// If this field is reset to zero, each interface within a configuration specifies its own
    /// class information and the various interfaces operate independently.
    ///
    /// If this field is set to a value between 1 and FEH, the device supports different class
    /// specifications on different interfaces and the interfaces may not operate independently.
    ///
    /// This value identifies the class definition used for the aggregate interfaces.
    /// If this field is set to FFH, the device class is vendor-specific.
    pub device_class: u8,

    /// Subclass code (assigned by the USB-IF).
    ///
    /// These codes are qualified by the value of the bDeviceClass field.
    /// If the bDeviceClass field is reset to zero, this field must also be reset to zero.
    /// If the bDeviceClass field is not set to FFH, all values are reserved for assignment by the USB-IF.
    pub device_sub_class: u8,

    /// Protocol code (assigned by the USB-IF).
    /// These codes are qualified by the value of the bDeviceClass and the bDeviceSubClass fields.
    ///
    /// If a device supports class-specific protocols on a device basis as opposed to an interface
    /// basis, this code identifies the protocols that the device uses as defined by the
    /// specification of the device class.
    /// If this field is reset to zero, the device does not use class-specific protocols on a
    /// device basis. However, it may use class- specific protocols on an interface basis.
    /// If this field is set to FFH, the device uses a vendor-specific protocol on a device basis.
    pub device_protocol: u8,

    /// Maximum packet size for endpoint zero
    ///
    /// (only 8, 16, 32, or 64 are valid)
    pub max_packet_size: u8,

    /// Vendor ID (assigned by the USB-IF)
    pub id_vendor: u16,

    /// Product ID (assigned by the manufacturer)
    pub id_product: u16,

    /// Device release number in binary-coded decimal
    pub device_release: Bcd16,

    /// Index of string descriptor describing manufacturer
    pub manufacturer_index: u8,

    /// Index of string descriptor describing product
    pub product_index: u8,

    /// Index of string descriptor describing the device's serial number
    pub serial_number_index: u8,

    /// Number of possible configurations
    pub num_configurations: u8,
}

/// The configuration descriptor describes information about a specific device configuration.
///
/// The descriptor contains a bConfigurationValue field with a value that, when used as a parameter
/// to the SetConfiguration() request, causes the device to assume the described configuration.
#[derive(Format)]
pub struct ConfigurationDescriptor {
    /// Total length of data returned for this configuration.
    ///
    /// Includes the combined length of all descriptors (configuration, interface,
    /// endpoint, and class- or vendor-specific) returned for this configuration.
    pub total_length: u16,

    /// Number of interfaces supported by this configuration
    pub num_interfaces: u8,

    /// Value to use as an argument to the SetConfiguration() request to select this configuration
    pub value: u8,

    /// Index of string descriptor describing this configuration
    pub index: u8,

    /// Configuration characteristics
    pub attributes: ConfigurationAttributes,

    /// Maximum power consumption of the USB device from the bus in this specific configuration when the device is fully operational.
    ///
    /// Expressed in 2 mA units (i.e., 50 = 100 mA).
    pub max_power: u8,
}

#[derive(Clone, Copy, Format)]
pub struct ConfigurationAttributes(u8);

/// Part of the [`ConfigurationDescriptor`]
impl ConfigurationAttributes {
    /// A device configuration reports whether the configuration is bus-powered or self-powered.
    ///
    /// Device status reports whether the device is currently self-powered. If a device is
    /// disconnected from its external power source, it updates device status to indicate that
    /// it is no longer self-powered.
    pub fn self_powered(&self) -> bool {
        (self.0 >> 6) & 1 == 1
    }

    /// Device supports remote wakeup
    pub fn remote_wakeup(&self) -> bool {
        (self.0 >> 5) & 1 == 1
    }
}

/// The interface descriptor describes a specific interface within a configuration. A configuration provides one
/// or more interfaces, each with zero or more endpoint descriptors describing a unique set of endpoints within
/// the configuration. When a configuration supports more than one interface, the endpoint descriptors for a
/// particular interface follow the interface descriptor in the data returned by the GetConfiguration() request.
/// An interface descriptor is always returned as part of a configuration descriptor. Interface descriptors cannot
/// be directly accessed with a GetDescriptor() or SetDescriptor() request.
#[derive(Format)]
pub struct InterfaceDescriptor {
    /// Number of this interface.
    ///
    /// Zero-based value identifying the index in the array of
    /// concurrent interfaces supported by this configuration.
    pub interface_number: u8,

    /// Value used to select this alternate setting for the interface identified in the prior field
    pub alternate_setting: u8,

    /// Number of endpoints used by this interface (excluding endpoint zero).
    ///
    /// If this value is zero, this interface only uses the Default Control Pipe.
    pub num_endpoints: u8,

    /// Class code (assigned by the USB-IF).
    ///
    /// A value of zero is reserved for future standardization.
    /// If this field is set to FFH, the interface class is vendor-specific.
    /// All other values are reserved for assignment by the USB-IF.
    pub interface_class: u8,

    /// Subclass code (assigned by the USB-IF).
    ///
    /// These codes are qualified by the value of the bInterfaceClass field.
    /// If the bInterfaceClass field is reset to zero, this field must also be reset to zero.
    /// If the bInterfaceClass field is not set to FFH, all values are reserved for assignment by the USB-IF.
    pub interface_sub_class: u8,

    /// Protocol code (assigned by the USB).
    ///
    /// These codes are qualified by the value of the bInterfaceClass and the
    /// bInterfaceSubClass fields. If an interface supports class-specific requests, this code
    /// identifies the protocols that the device uses as defined by the specification of the
    /// device class.
    /// If this field is reset to zero, the device does not use a class-specific protocol on
    /// this interface.
    /// If this field is set to FFH, the device uses a vendor-specific protocol for this interface.
    pub interface_protocol: u8,

    /// Index of string descriptor describing this interface
    pub interface_index: u8,
}

/// Each endpoint used for an interface has its own descriptor.
///
/// This descriptor contains the information required by the host to determine the bandwidth requirements of each endpoint.
#[derive(Format)]
pub struct EndpointDescriptor {
    /// The address of the endpoint on the USB device described by this descriptor.
    pub address: EndpointAddress,

    /// This field describes the endpoint’s attributes when it is configured using the bConfigurationValue.
    pub attributes: EndpointAttributes,

    /// Maximum packet size this endpoint is capable of sending or receiving when this configuration is selected.
    pub max_packet_size: u16,

    /// Interval for polling endpoint for data transfers.
    ///
    /// Expressed in frames (1 millisecond).
    pub interval: u8,
}

#[derive(Clone, Copy, Format)]
/// Address of an endpoint
///
/// Part of an [`EndpointDescriptor`].
pub struct EndpointAddress(u8);

impl EndpointAddress {
    /// Endpoint number
    ///
    /// Ranges from 1 to 15.
    pub fn number(&self) -> u8 {
        self.0 & 0b111
    }

    /// Direction of the endpoint
    pub fn direction(&self) -> UsbDirection {
        self.0.into()
    }
}

#[derive(Clone, Copy, Format)]
/// Attributes of an endpoint
///
/// Part of an [`EndpointDescriptor`].
pub struct EndpointAttributes(u8);

impl EndpointAttributes {
    pub fn transfer_type(&self) -> TransferType {
        unsafe { core::mem::transmute(self.0 & 0b11) }
    }

    /// Synchronization type. Only valid for Isochronous endpoint.
    pub fn synchronization_type(&self) -> SynchronizationType {
        unsafe { core::mem::transmute((self.0 >> 2) & 0b11) }
    }

    /// Usage type. Only valid for Isochronous endpoint.
    pub fn usage_type(&self) -> UsageType {
        unsafe { core::mem::transmute((self.0 >> 4) & 0b11) }
    }
}

#[derive(Clone, Copy)]
#[repr(u8)]
/// Synchronization type for an Isochronous endpoint
pub enum SynchronizationType {
    NoSynchronization = 0b00,
    Asynchronouse = 0b01,
    Adaptive = 0b10,
    Synchronous = 0b11,
}

#[derive(Clone, Copy)]
#[repr(u8)]
/// Usage type for an Isochronous endpoint
pub enum UsageType {
    Data = 0b00,
    Feedback = 0b01,
    ImplicitFeedbackData = 0b10,
    Reserved = 0b11,
}

pub mod parse {
    use nom::IResult;
    use nom::combinator::{map, verify};
    use nom::sequence::tuple;
    use nom::bytes::streaming::take;
    use nom::number::streaming::{u8, le_u16};

    use super::*;

    /// Parse outer framing of a descriptor
    ///
    /// The resulting `data` within the descriptor can then be parsed with one of the other functions below,
    /// depending on the `type`.
    pub fn any_descriptor(input: &[u8]) -> IResult<&[u8], Descriptor<'_>> {
        let (input, (length, descriptor_type)) = tuple((u8, u8))(input)?;
        let (input, data) = take((length - 2) as usize)(input)?;
        Ok((input, Descriptor { length, descriptor_type, data }))
    }

    /// Parse descriptor data for a device
    pub fn device_descriptor(input: &[u8]) -> IResult<&[u8], DeviceDescriptor> {
        map(
            tuple((bcd_16, u8, u8, u8, u8, le_u16, le_u16, bcd_16, u8, u8, u8, u8)),
            |(usb_release, device_class, device_sub_class, device_protocol, max_packet_size,
              id_vendor, id_product, device_release, manufacturer_index, product_index,
              serial_number_index, num_configurations)| {
                DeviceDescriptor {
                    usb_release, device_class, device_sub_class, device_protocol, max_packet_size,
                    id_vendor, id_product, device_release, manufacturer_index, product_index,
                    serial_number_index, num_configurations,
                }
            }
        )(input)
    }

    /// Parse descriptor data for a configuration
    pub fn configuration_descriptor(input: &[u8]) -> IResult<&[u8], ConfigurationDescriptor> {
        map(
            tuple((le_u16, u8, u8, u8, u8, u8)),
            |(total_length, num_interfaces, value, index, attributes, max_power)| {
                ConfigurationDescriptor {
                    total_length, num_interfaces, value, index,
                    attributes: ConfigurationAttributes(attributes),
                    max_power,
                }
            }
        )(input)
    }

    /// Parse only the `total_length` from a (partial) configuration descriptor
    pub fn configuration_descriptor_length(input: &[u8]) -> IResult<&[u8], u16> {
        le_u16(input)
    }

    /// Parse descriptor data for an interface
    pub fn interface_descriptor(input: &[u8]) -> IResult<&[u8], InterfaceDescriptor> {
        map(
            tuple((u8, u8, u8, u8, u8, u8, u8)),
            |(interface_number, alternate_setting, num_endpoints, interface_class, interface_sub_class,
              interface_protocol, interface_index)| {
                InterfaceDescriptor {
                    interface_number, alternate_setting, num_endpoints, interface_class, interface_sub_class,
                    interface_protocol, interface_index,
                }
            }
        )(input)
    }

    /// Parse descriptor data for an endpoint
    pub fn endpoint_descriptor(input: &[u8]) -> IResult<&[u8], EndpointDescriptor> {
        map(
            tuple((u8, u8, le_u16, u8)),
            |(address, attributes, max_packet_size, interval)| {
                EndpointDescriptor {
                    address: EndpointAddress(address),
                    attributes: EndpointAttributes(attributes),
                    max_packet_size,
                    interval,
                }
            }
        )(input)
    }

    /// Parses a 16-bit binary coded decimal value
    ///
    /// Succeeds only if the data is indeed a valid value. This requires all four nibbles (i.e. half-bytes) to be in the 0-9 range.
    pub fn bcd_16(input: &[u8]) -> IResult<&[u8], Bcd16> {
        map(verify(le_u16, |value| Bcd16::is_valid(*value)), Bcd16)(input)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_any_descriptor() {
            let data = [8, 7, 6, 5, 4, 3, 2, 1, 0];
            let (rest, desc) = any_descriptor(&data).unwrap();
            assert_eq!(desc.length, 8);
            assert_eq!(desc.descriptor_type, 7);
            assert_eq!(desc.data, &[6, 5, 4, 3, 2, 1]);
            assert_eq!(rest, &[0]);
        }

        #[test]
        fn test_bcd_16() {
            let (_, Bcd16(bcd)) = bcd_16(&[0x10, 0x02]).unwrap();
            assert_eq!(bcd, 0x0210);

            assert!(bcd_16(&[0x00, 0x01]).is_ok());
            assert!(bcd_16(&[0x00, 0x02]).is_ok());
            assert!(bcd_16(&[0x00, 0x03]).is_ok());
            assert!(bcd_16(&[0x00, 0x04]).is_ok());
            assert!(bcd_16(&[0x00, 0x05]).is_ok());
            assert!(bcd_16(&[0x00, 0x06]).is_ok());
            assert!(bcd_16(&[0x00, 0x07]).is_ok());
            assert!(bcd_16(&[0x00, 0x08]).is_ok());
            assert!(bcd_16(&[0x00, 0x09]).is_ok());
            assert!(bcd_16(&[0x00, 0x0A]).is_err());
            assert!(bcd_16(&[0x00, 0x0B]).is_err());
            assert!(bcd_16(&[0x00, 0x0C]).is_err());
            assert!(bcd_16(&[0x00, 0x0D]).is_err());
            assert!(bcd_16(&[0x00, 0x0E]).is_err());
            assert!(bcd_16(&[0x00, 0x0F]).is_err());
        }
    }
}
