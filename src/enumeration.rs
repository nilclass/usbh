use crate::types::{
    DeviceAddress,
    DescriptorType,
};
use crate::bus::HostBus;
use crate::{UsbHost, Event};
use usb_device::control::Recipient;
use defmt::Format;

#[derive(Copy, Clone, Format)]
pub enum EnumerationState {
    /// No device is attached yet
    WaitForDevice,
    /// Device was attached, bus was reset, waiting for the device to appear again
    Reset0,
    /// Device has appeared, wait for a little while
    Delay0,
    /// Have sent initial GET_DESCRIPTOR to addr (0, 0), waiting for a reply
    WaitDescriptor,
    /// Bus was reset for the second time, waiting for the device to appear again
    Reset1,
    /// Device has appeared again, wait for a little while until setting address
    Delay1,
    /// Device has reappeared, SET_ADDRESS was sent, waiting for a reply
    WaitSetAddress(DeviceAddress),
    /// Device now has an address assigned, enumeration is done.
    Assigned(DeviceAddress),
}

impl EnumerationState {
    pub(crate) fn delay(&self) -> Option<fugit::MillisDurationU32> {
        match self {
            EnumerationState::Delay0 => Some(fugit::MillisDurationU32::millis(20)),
            EnumerationState::Delay1 => Some(fugit::MillisDurationU32::millis(10)),
            _ => None,
        }
    }
}

pub fn process_enumeration<B: HostBus>(event: Event, state: EnumerationState, host: &mut UsbHost<B>) -> EnumerationState {
    match state {
        EnumerationState::WaitForDevice => {
            match event {
                Event::Attached(_) => {
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
                    EnumerationState::Delay0
                }
                _ => state,
            }
        }

        EnumerationState::Delay0 => {
            match event {
                Event::DelayComplete => {
                    host.get_descriptor(None, Recipient::Device, DescriptorType::Device, 8);
                    EnumerationState::WaitDescriptor
                },
                Event::Detached => EnumerationState::WaitForDevice,
                _ => state,
            }
        }

        EnumerationState::WaitDescriptor => {
            match event {
                Event::Detached => EnumerationState::WaitForDevice,
                Event::ControlInData => {
                    host.bus.reset_bus();
                    EnumerationState::Reset1
                }
                _ => state,
            }
        },

        EnumerationState::Reset1 => {
            match event {
                Event::Attached(_) => {
                    host.bus.enable_sof();
                    EnumerationState::Delay1
                }
                // TODO: handle timeouts
                _ => state,
            }
        },

        EnumerationState::Delay1 => {
            match event {
                Event::DelayComplete => {
                    let address = host.next_address();
                    host.set_address(address);
                    EnumerationState::WaitSetAddress(address)
                },
                Event::Detached => EnumerationState::WaitForDevice,
                _ => state,
            }
        }

        EnumerationState::WaitSetAddress(address) => {
            match event {
                Event::Detached => EnumerationState::WaitForDevice,
                Event::ControlOutComplete => EnumerationState::Assigned(address),
                _ => state,
            }
        },

        EnumerationState::Assigned(_) => unreachable!(),
    }
}