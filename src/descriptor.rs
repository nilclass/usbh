use crate::types::{Bcd16, TransferType};
use usb_device::UsbDirection;

pub struct Descriptor<'a> {
    pub length: u8,
    pub descriptor_type: u8,
    pub data: &'a [u8],
}

/// A device descriptor describes general information about a USB device. It includes information that applies
/// globally to the device and all of the device’s configurations. A USB device has only one device descriptor.
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

#[derive(Clone, Copy)]
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

#[derive(Clone, Copy)]
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

#[derive(Clone, Copy)]
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
    use nom::combinator::map;
    use nom::sequence::tuple;
    use nom::bytes::streaming::take;
    use nom::number::streaming::{u8, le_u16};

    use super::*;

    pub fn any_descriptor(input: &[u8]) -> IResult<&[u8], Descriptor<'_>> {
        let (input, (length, descriptor_type)) = tuple((u8, u8))(input)?;
        let (input, data) = take((length - 2) as usize)(input)?;
        Ok((input, Descriptor { length, descriptor_type, data }))
    }

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

    pub fn configuration_descriptor_length(input: &[u8]) -> IResult<&[u8], u16> {
        le_u16(input)
    }

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

    pub fn bcd_16(input: &[u8]) -> IResult<&[u8], Bcd16> {
        map(le_u16, Bcd16)(input)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_bcd_16() {
            let (_, Bcd16(bcd)) = bcd_16(&[0x10, 0x02]).unwrap();
            assert_eq!(bcd, 0x0210);
        }

        // #[test]
        // fn test_device_descriptor(
    }
}
