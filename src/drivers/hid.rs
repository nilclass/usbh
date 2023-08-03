use core::num::NonZeroU8;
use crate::{UsbHost, WouldBlock};
use crate::bus::HostBus;
use crate::types::DeviceAddress;
use crate::pipe::{ControlPipe, InterruptPipe};
use usb_device::control::{RequestType, Recipient};

const GET_REPORT: u8 = 0x01;
const GET_IDLE: u8 = 0x02;
const GET_PROTOCOL: u8 = 0x03;
const SET_REPORT: u8 = 0x09;
const SET_IDLE: u8 = 0x0A;
const SET_PROTOCOL: u8 = 0x0B;


#[repr(u8)]
enum ReportType {
    Input = 0x01,
    Output = 0x02,
    Feature = 0x03,
}

struct BootKeyboard<B> {
    control: ControlPipe<B>,
    interrupt: InterruptPipe<B>,
    interface: u16,
    state: BootKeyboardState,
}

enum BootKeyboardState {
    Idle,
    GetInputReport,
    GetOutputReport,
    SetOutputReport,
}

enum BootKeyboardEvent {
    InputReport(InputReport),
    OutputReport(OutputReport),
    SetOutputReportDone,
    Interrupt(InputReport),
    Error(BootKeyboardError),
}

enum BootKeyboardError {
    InvalidData,
}

impl<B: HostBus> BootKeyboard<B> {
    fn new(device_address: DeviceAddress, interface: u16, interrupt_ep: u8, host: &mut UsbHost<B>) -> Self {
        Self {
            control: host.create_control_pipe(device_address),
            interrupt: host.create_interrupt_in_pipe(device_address, interrupt_ep),
            interface,
            state: BootKeyboardState::Idle,
        }
    }

    fn get_input_report(&mut self) -> Result<(), WouldBlock> {
        self.control.transfer_in(
            RequestType::Class,
            Recipient::Interface,
            GET_REPORT,
            (ReportType::Input as u8 as u16) << 8,
            self.interface,
            8,
        )
    }

    fn get_output_report(&mut self) -> Result<(), WouldBlock> {
        self.control.transfer_in(
            RequestType::Class,
            Recipient::Interface,
            GET_REPORT,
            (ReportType::Output as u8 as u16) << 8,
            self.interface,
            1,
        )
    }

    fn set_output_report(&mut self, report: OutputReport) -> Result<(), WouldBlock> {
        self.control.transfer_out(
            RequestType::Class,
            Recipient::Interface,
            SET_REPORT,
            (ReportType::Output as u8 as u16) << 8,
            self.interface,
            &[report.0],
        )
    }

    fn poll(&mut self) -> Option<BootKeyboardEvent> {
        match self.state {
            BootKeyboardState::Idle => {
                self.interrupt.poll().map(|data| {
                    match InputReport::try_from(data) {
                        Ok(input_report) => BootKeyboardEvent::InputReport(input_report),
                        Err(_) => BootKeyboardEvent::Error(BootKeyboardError::InvalidData),
                    }
                })
            },
            BootKeyboardState::GetInputReport => {
                self.control.complete_in().map(|data| {
                    match InputReport::try_from(data) {
                        Ok(input_report) => BootKeyboardEvent::InputReport(input_report),
                        Err(_) => BootKeyboardEvent::Error(BootKeyboardError::InvalidData),
                    }
                })
            },
            BootKeyboardState::GetOutputReport => {
                self.control.complete_in().map(|data| {
                    if data.len() == 1 {
                        BootKeyboardEvent::OutputReport(data[0])
                    } else {
                        BootKeyboardEvent::Error(BootKeyboardError::InvalidData)
                    }
                })
            },
            BootKeyboardState::SetOutputReport => {
                self.control.complete_out().map(|| BootKeyboardEvent::SetOutputReportDone)
            },
        }
    }
}

#[derive(Debug)]
#[repr(packed)]
pub struct InputReport {
    modifier_status: ModifierStatus,
    _reserved: u8,
    keypress_1: Option<NonZeroU8>,
    keypress_2: Option<NonZeroU8>,
    keypress_3: Option<NonZeroU8>,
    keypress_4: Option<NonZeroU8>,
    keypress_5: Option<NonZeroU8>,
    keypress_6: Option<NonZeroU8>,
}

impl<'a> TryFrom<&'a [u8]> for &'a InputReport {
    type Error = ();

    fn try_from(value: &'a [u8]) -> Result<Self, Self::Error> {
        if value.len() == 8 && core::mem::size_of::<InputReport>() == 8 {
            // Safety: we have verified that the InputReport struct and the provided value have the expected size
            Ok(unsafe { &*(value as *const _ as *const InputReport) })
        } else {
            Err(())
        }
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct OutputReport(u8);

impl OutputReport {
    pub fn num_lock(self) -> bool {
        self.0 & 1 == 1
    }

    pub fn set_num_lock(mut self, on: bool) {
        if on {
            self.0 |= 1;
        } else {
            self.0 &= !1;
        }
    }

    pub fn caps_lock(self) -> bool {
        (self.0 >> 1) & 1 == 1
    }

    pub fn set_caps_lock(mut self, on: bool) {
        if on {
            self.0 |= 1 << 1;
        } else {
            self.0 &= !(1 << 1);
        }
    }

    pub fn scroll_lock(self) -> bool {
        (self.0 >> 2) & 1 == 1
    }

    pub fn set_scroll_lock(mut self, on: bool) {
        if on {
            self.0 |= 1 << 2;
        } else {
            self.0 &= !(1 << 2);
        }
    }

    pub fn compose(self) -> bool {
        (self.0 >> 3) & 1 == 1
    }

    pub fn set_compose(mut self, on: bool) {
        if on {
            self.0 |= 1 << 3;
        } else {
            self.0 &= !(1 << 3);
        }
    }

    pub fn kana(self) -> bool {
        (self.0 >> 4) & 1 == 1
    }

    pub fn set_kana(mut self, on: bool) {
        if on {
            self.0 |= 1 << 4;
        } else {
            self.0 &= !(1 << 4);
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ModifierStatus(u8);

impl ModifierStatus {
    pub fn left_ctrl(&self) -> bool {
        self.0 & 1 == 1
    }
    pub fn left_shift(&self) -> bool {
        (self.0 >> 1) & 1 == 1
    }
    pub fn left_alt(&self) -> bool {
        (self.0 >> 2) & 1 == 1
    }
    pub fn left_gui(&self) -> bool {
        (self.0 >> 3) & 1 == 1
    }
    pub fn right_ctrl(&self) -> bool {
        (self.0 >> 4) & 1 == 1
    }
    pub fn right_shift(&self) -> bool {
        (self.0 >> 5) & 1 == 1
    }
    pub fn right_alt(&self) -> bool {
        (self.0 >> 6) & 1 == 1
    }
    pub fn right_gui(&self) -> bool {
        (self.0 >> 7) & 1 == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_report_from_bytes() {
        let bytes: [u8; 8] = [0x02, 0x00, 0x1B, 0x00, 0x00, 0x00, 0x00, 0x00];
        let input_report: &InputReport = bytes[..].try_into().unwrap();
        assert_eq!(input_report.modifier_status.left_ctrl(), false);
        assert_eq!(input_report.modifier_status.left_shift(), true);
        assert_eq!(input_report.modifier_status.left_alt(), false);
        assert_eq!(input_report.modifier_status.left_gui(), false);
        assert_eq!(input_report.modifier_status.right_ctrl(), false);
        assert_eq!(input_report.modifier_status.right_shift(), false);
        assert_eq!(input_report.modifier_status.right_alt(), false);
        assert_eq!(input_report.modifier_status.right_gui(), false);
        assert_eq!(input_report.keypress_1, NonZeroU8::new(0x1B));
        assert_eq!(input_report.keypress_2, None);
        assert_eq!(input_report.keypress_3, None);
        assert_eq!(input_report.keypress_4, None);
        assert_eq!(input_report.keypress_5, None);
        assert_eq!(input_report.keypress_6, None);
    }

    #[test]
    fn test_output_report() {
    }
}
