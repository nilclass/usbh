use crate::types::{
    DeviceAddress,
    DescriptorType,
};
use crate::bus::HostBus;
use crate::{UsbHost, Event};
use usb_device::control::Recipient;
use crate::driver::Driver;
use crate::descriptor;

#[derive(Copy, Clone)]
pub enum DiscoveryState {
    // get device descriptor
    DeviceDesc,
    // get configuration descriptor length n of m
    ConfigDescLen(u8, u8),
    // get full configuration descriptor n of m
    ConfigDesc(u8, u8),
    // finished discovery.
    Done,
}

pub fn start_discovery<B: HostBus>(dev_addr: DeviceAddress, host: &mut UsbHost<B>) -> DiscoveryState {
    host.get_descriptor(Some(dev_addr), Recipient::Device, DescriptorType::Device, 0, 18).unwrap();
    DiscoveryState::DeviceDesc
}

pub fn process_discovery<B: HostBus>(event: Event, dev_addr: DeviceAddress, state: DiscoveryState, driver: &mut dyn Driver<B>, host: &mut UsbHost<B>) -> DiscoveryState {
    match state {
        DiscoveryState::DeviceDesc => {
            match event {
                Event::ControlInData(_, length) => {
                    let data = unsafe { host.bus.control_buffer(length as usize) };
                    let (_, descriptor) = descriptor::parse::any_descriptor(data).unwrap();
                    driver.descriptor(dev_addr, descriptor.descriptor_type, descriptor.data);
                    let (_, device_descriptor) = descriptor::parse::device_descriptor(descriptor.data).unwrap();

                    host.get_descriptor(Some(dev_addr), Recipient::Device, DescriptorType::Configuration, 0, 9).unwrap();
                    DiscoveryState::ConfigDescLen(0, device_descriptor.num_configurations)
                }
                _ => state
            }
        },
        DiscoveryState::ConfigDescLen(n, m) => {
            match event {
                Event::ControlInData(_, length) => {
                    let data = unsafe { host.bus.control_buffer(length as usize) };
                    let (_, descriptor) = descriptor::parse::any_descriptor(data).unwrap();
                    let (_, total_length) = descriptor::parse::configuration_descriptor_length(descriptor.data).unwrap();
                    host.get_descriptor(Some(dev_addr), Recipient::Device, DescriptorType::Configuration, n, total_length).unwrap();
                    DiscoveryState::ConfigDesc(n, m)
                }
                _ => state
            }
        },
        DiscoveryState::ConfigDesc(n, m) => {
            match event {
                Event::ControlInData(_, length) => {
                    let mut data = unsafe { host.bus.control_buffer(length as usize) };
                    loop {
                        let (rest, descriptor) = descriptor::parse::any_descriptor(data).unwrap();
                        driver.descriptor(dev_addr, descriptor.descriptor_type, descriptor.data);
                        if rest.len() > 0 {
                            data = rest;
                        } else {
                            break;
                        }
                    }
                    if (n + 1) < m {
                        host.get_descriptor(Some(dev_addr), Recipient::Device, DescriptorType::Configuration, n + 1, 9).unwrap();
                        DiscoveryState::ConfigDesc(n + 1, m)
                    } else {
                        DiscoveryState::Done
                    }
                }
                _ => state
            }
        },
        DiscoveryState::Done => unreachable!(),
    }
}
