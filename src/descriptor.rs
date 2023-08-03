use crate::types::{DescriptorType, Bcd16, TransferType};

pub struct Descriptor<'a> {
    pub length: u8,
    pub descriptor_type: u8,
    pub data: &'a [u8],
}

pub struct DeviceDescriptor {
    pub usb_release: Bcd16,
    pub device_class: u8,
    pub device_sub_class: u8,
    pub device_protocol: u8,
    pub max_packet_size: u8,
    pub id_vendor: u16,
    pub id_product: u16,
    pub device_release: Bcd16,
    pub manufacturer_index: u8,
    pub product_index: u8,
    pub serial_number_index: u8,
    pub num_configurations: u8,
}

pub struct ConfigurationDescriptor {
    pub total_length: u16,
    pub num_interfaces: u8,
    pub value: u8,
    pub index: u8,
    pub attributes: ConfigurationAttributes,
    pub max_power: u8,
}

#[derive(Clone, Copy)]
pub struct ConfigurationAttributes(u8);

impl ConfigurationAttributes {
    pub fn self_powered(&self) -> bool {
        (self.0 >> 6) & 1 == 1
    }

    pub fn remote_wakeup(&self) -> bool {
        (self.0 >> 5) & 1 == 1
    }
}

pub struct InterfaceDescriptor {
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub num_endpoints: u8,
    pub interface_class: u8,
    pub interface_sub_class: u8,
    pub interface_protocol: u8,
    pub interface_index: u8,
}

pub struct EndpointDescriptor {
    pub address: u8,
    pub attributes: EndpointAttributes,
    pub max_packet_size: u16,
    pub interval: u8,
}

#[derive(Clone, Copy)]
pub struct EndpointAttributes(u8);

impl EndpointAttributes {
    pub fn transfer_type(&self) -> TransferType {
        match self.0 & 0b11 {
            0b00 => TransferType::Control,
            0b01 => TransferType::Isochronous,
            0b10 => TransferType::Bulk,
            0b11 => TransferType::Interrupt,
            _ => unreachable!(),
        }
    }
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
        let (input, data) = take(length as usize)(input)?;
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
                    address,
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
