use crate::bus::HostBus;
use crate::{UsbHost, Event};
use crate::types::{DeviceAddress, ConnectionSpeed};
use crate::descriptor;
use usb_device::control::Recipient;
use defmt::{debug, Format};

#[derive(Copy, Clone, Format)]
pub enum EnumerationState {
    /// No device is attached yet
    WaitForDevice,
    /// Device was attached, bus was reset, waiting for the device to appear again
    Reset0,
    /// Device has appeared, wait for a little while
    Delay0(u8),
    /// Have sent initial GET_DESCRIPTOR to addr (0, 0), waiting for a reply
    WaitDescriptor,
    /// Bus was reset for the second time, waiting for the device to appear again
    Reset1,
    /// Device has appeared again, wait for a little while until setting address
    Delay1(ConnectionSpeed, u8),
    /// Device has reappeared, SET_ADDRESS was sent, waiting for a reply
    WaitSetAddress(ConnectionSpeed, DeviceAddress),
    /// Device now has an address assigned, enumeration is done.
    Assigned(ConnectionSpeed, DeviceAddress),
}

const RESET_0_DELAY: u8 = 10;
const RESET_1_DELAY: u8 = 10;

pub fn process_enumeration<B: HostBus>(event: Event, state: EnumerationState, host: &mut UsbHost<B>) -> EnumerationState {
    match state {
        EnumerationState::WaitForDevice => {
            match event {
                Event::Attached(_) => {
                    debug!("[UsbHost enumeration] -> Reset0");
                    host.bus.reset_bus();
                    EnumerationState::Reset0
                },
                // TODO: handle timeouts
                _ => state,
            }
        }

        EnumerationState::Reset0 => {
            match event {
                Event::Attached(_) => {
                    host.bus.enable_sof();
                    debug!("[UsbHost enumeration] -> Delay0");
                    host.bus.interrupt_on_sof(true);
                    EnumerationState::Delay0(RESET_0_DELAY)
                }
                _ => state,
            }
        }

        EnumerationState::Delay0(n) => {
            match event {
                Event::Sof => {
                    if n > 0 {
                        EnumerationState::Delay0(n - 1)
                    } else {
                        host.get_descriptor(None, None, Recipient::Device, descriptor::TYPE_DEVICE, 0, 8);
                        debug!("[UsbHost enumeration] -> WaitDescriptor");
                        EnumerationState::WaitDescriptor
                    }
                },
                Event::Detached => EnumerationState::WaitForDevice,
                _ => state,
            }
        }

        EnumerationState::WaitDescriptor => {
            match event {
                Event::Detached => {
                    debug!("[UsbHost enumeration] -> WaitForDevice");
                    host.bus.interrupt_on_sof(false);
                    EnumerationState::WaitForDevice
                },
                Event::ControlInData(_, _) => {
                    debug!("[UsbHost enumeration] -> Reset1");
                    host.bus.reset_bus();
                    EnumerationState::Reset1
                }
                _ => state,
            }
        },

        EnumerationState::Reset1 => {
            match event {
                Event::Attached(speed) => {
                    host.bus.enable_sof();
                    debug!("[UsbHost enumeration] -> Delay1");
                    EnumerationState::Delay1(speed, RESET_1_DELAY)
                }
                // TODO: handle timeouts
                _ => state,
            }
        },

        EnumerationState::Delay1(speed, n) => {
            match event {
                Event::Sof => {
                    if n > 0 {
                        EnumerationState::Delay1(speed, n - 1)
                    } else {
                        let address = host.next_address();
                        host.set_address(address);
                        debug!("[UsbHost enumeration] -> WaitSetAddress");
                        EnumerationState::WaitSetAddress(speed, address)
                    }
                },
                Event::Detached => {
                    debug!("[UsbHost enumeration] -> WaitForDevice");
                    host.bus.interrupt_on_sof(false);
                    EnumerationState::WaitForDevice
                },
                _ => state,
            }
        }

        EnumerationState::WaitSetAddress(speed, address) => {
            match event {
                Event::Detached => {
                    debug!("[UsbHost enumeration] -> WaitForDevice");
                    host.bus.interrupt_on_sof(false);
                    EnumerationState::WaitForDevice
                },
                Event::ControlOutComplete(_) => {
                    debug!("[UsbHost enumeration] -> Assigned({})", address);
                    host.bus.interrupt_on_sof(false);
                    EnumerationState::Assigned(speed, address)
                },
                _ => state,
            }
        },

        EnumerationState::Assigned(_speed, _address) => unreachable!(),
    }
}
