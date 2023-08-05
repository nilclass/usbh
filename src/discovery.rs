use crate::types::DeviceAddress;
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
    // failed to parse one of the descriptors
    ParseError,
}

/// Begin discovery, by requesting the device descriptor
pub fn start_discovery<B: HostBus>(dev_addr: DeviceAddress, host: &mut UsbHost<B>) -> DiscoveryState {
    // Unwrap safety: it is up to the UsbHost to start discovery only when no other transfer is in progress.
    host.get_descriptor(Some(dev_addr), None, Recipient::Device, descriptor::TYPE_DEVICE, 0, 18).ok().unwrap();
    DiscoveryState::DeviceDesc
}

pub fn process_discovery<B: HostBus>(event: Event, dev_addr: DeviceAddress, state: DiscoveryState, drivers: &mut [&mut dyn Driver<B>], host: &mut UsbHost<B>) -> DiscoveryState {
    match state {
        DiscoveryState::DeviceDesc => {
            match event {
                Event::ControlInData(_, length) => {
                    let data = unsafe { host.bus.control_buffer(length as usize) };
                    let Ok((_, descriptor)) = descriptor::parse::any_descriptor(data) else {
                        return DiscoveryState::ParseError
                    };
                    for driver in drivers {
                        driver.descriptor(dev_addr, descriptor.descriptor_type, descriptor.data);
                    }
                    let Ok((_, device_descriptor)) = descriptor::parse::device_descriptor(descriptor.data) else {
                        return DiscoveryState::ParseError
                    };

                    // Unwrap safety: when a `Control*` event is emitted, the host is idle and a transfer can be started
                    host.get_descriptor(Some(dev_addr), None, Recipient::Device, descriptor::TYPE_CONFIGURATION, 0, 9).ok().unwrap();
                    DiscoveryState::ConfigDescLen(0, device_descriptor.num_configurations)
                }
                _ => state
            }
        },
        DiscoveryState::ConfigDescLen(n, m) => {
            match event {
                Event::ControlInData(_, length) => {
                    let data = unsafe { host.bus.control_buffer(length as usize) };
                    let Ok((_, descriptor)) = descriptor::parse::any_descriptor(data) else {
                        return DiscoveryState::ParseError
                    };
                    let Ok((_, total_length)) = descriptor::parse::configuration_descriptor_length(descriptor.data) else {
                        return DiscoveryState::ParseError
                    };
                    // Unwrap safety: when a `Control*` event is emitted, the host is idle and a transfer can be started
                    host.get_descriptor(Some(dev_addr), None, Recipient::Device, descriptor::TYPE_CONFIGURATION, n, total_length).ok().unwrap();
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
                        let Ok((rest, descriptor)) = descriptor::parse::any_descriptor(data) else {
                            return DiscoveryState::ParseError
                        };
                        for driver in &mut *drivers {
                            driver.descriptor(dev_addr, descriptor.descriptor_type, descriptor.data);
                        }
                        if rest.len() > 0 {
                            data = rest;
                        } else {
                            break;
                        }
                    }
                    if (n + 1) < m {
                        // Unwrap safety: when a `Control*` event is emitted, the host is idle and a transfer can be started
                        host.get_descriptor(Some(dev_addr), None, Recipient::Device, descriptor::TYPE_CONFIGURATION, n + 1, 9).ok().unwrap();
                        DiscoveryState::ConfigDesc(n + 1, m)
                    } else {
                        // NOTE: do not start a transfer here, the UsbHost code expects the bus to stay idle.
                        DiscoveryState::Done
                    }
                }
                _ => state
            }
        },
        DiscoveryState::Done | DiscoveryState::ParseError => unreachable!(),
    }
}
