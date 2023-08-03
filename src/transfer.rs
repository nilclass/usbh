use crate::bus::HostBus;
use crate::UsbHost;
use usb_device::UsbDirection;

pub struct Transfer {
    length: u16,
    state: TransferState,
}

enum TransferState {
    Control(UsbDirection, ControlState),
    Interrupt(UsbDirection),
}

enum ControlState {
    WaitSetup,
    WaitData,
    WaitConfirm,
}

pub enum PollResult {
    ControlInComplete(u16),
    ControlOutComplete,
    InterruptInComplete(u16),
    InterruptOutComplete,
    Continue(Transfer),
}

impl Transfer {
    pub(crate) fn new_control_in(length: u16) -> Self {
        Self {
            length,
            state: TransferState::Control(UsbDirection::In, ControlState::WaitSetup),
        }
    }

    pub fn new_control_out(length: u16) -> Self {
        Self {
            length,
            state: TransferState::Control(UsbDirection::Out, ControlState::WaitSetup),
        }
    }

    pub fn new_interrupt_in(length: u16) -> Self {
        Self {
            length,
            state: TransferState::Interrupt(UsbDirection::In),
        }
    }

    pub(crate) fn stage_complete<B: HostBus>(self, host: &mut UsbHost<B>) -> PollResult {
        match self {
            Transfer { state: TransferState::Control(UsbDirection::In, control_state), length } => {
                match control_state {
                    ControlState::WaitSetup => {
                        host.bus.write_data_in(length, true);
                        PollResult::Continue(Transfer { state: TransferState::Control(UsbDirection::In, ControlState::WaitData), length })
                    }
                    ControlState::WaitData => {
                        host.bus.write_data_out(&[]);
                        PollResult::Continue(Transfer { state: TransferState::Control(UsbDirection::In, ControlState::WaitConfirm), length })
                    }
                    ControlState::WaitConfirm => {
                        PollResult::ControlInComplete(length)
                    },
                }
            }
            Transfer { state: TransferState::Control(UsbDirection::Out, control_state), length } => {
                match control_state {
                    ControlState::WaitSetup => {
                        if length == 0 {
                            host.bus.write_data_in(0, true);
                            PollResult::Continue(Transfer { state: TransferState::Control(UsbDirection::Out, ControlState::WaitConfirm), length })
                        } else {
                            host.bus.write_data_out_prepared();
                            PollResult::Continue(Transfer { state: TransferState::Control(UsbDirection::Out, ControlState::WaitData), length })
                        }
                    },
                    ControlState::WaitData => {
                        host.bus.write_data_in(0, true);
                        PollResult::Continue(Transfer { state: TransferState::Control(UsbDirection::Out, ControlState::WaitConfirm), length })
                    },
                    ControlState::WaitConfirm => {
                        PollResult::ControlOutComplete
                    },
                }
            }
            Transfer { state: TransferState::Interrupt(UsbDirection::In), length } => PollResult::InterruptInComplete(length),
            Transfer { state: TransferState::Interrupt(UsbDirection::Out), .. } => PollResult::InterruptOutComplete,
        }
    }
}
