use input::*;
use smart_default::SmartDefault;

#[derive(SmartDefault)]
pub(crate) struct Config {
    pub accel_config: Option<fn() -> AccelConfig>,
    pub profile_override: Option<AccelProfile>,
    pub accel_speed: Option<f64>,
    pub calibration_matrix: Option<[f32; 6]>,
    pub area_rectangle: Option<AreaRectangle>,
    #[default(Some(ClickMethod::Clickfinger))]
    pub click_method: Option<ClickMethod>,
    pub clickfinger_button_map: Option<ClickfingerButtonMap>,
    pub dwt_enabled: Option<bool>,
    pub dwtp_enabled: Option<bool>,
    pub left_handed: Option<bool>,
    pub middle_emulation_enabled: Option<bool>,
    pub rotation_angle: Option<u32>,
    pub scroll_button: Option<u32>,
    #[default(Some(ScrollMethod::TwoFinger))]
    pub scroll_method: Option<ScrollMethod>,
    pub natural_scroll_enabled: Option<bool>,
    pub scroll_button_lock: Option<ScrollButtonLockState>,
    pub send_events_mode: Option<SendEventsMode>,
    pub tap_button_map: Option<TapButtonMap>,
    pub tap_drag_enabled: Option<bool>,
    pub tap_drag_lock_enabled: Option<DragLockState>,
    pub tap_enabled: Option<bool>,
    pub threefg_drag_enabled: Option<ThreeFingerDragState>,
}

impl Config {
    pub fn apply(&self, device: &mut Device) {
        if let Some(constructor) = self.accel_config {
            let _ = device.config_accel_apply(constructor());
        }
        if let Some(profile) = self.profile_override {
            let _ = device.config_accel_set_profile(profile);
        }
        if let Some(accel_speed) = self.accel_speed {
            let _ = device.config_accel_set_speed(accel_speed);
        }
        if let Some(calibration_matrix) = self.calibration_matrix {
            let _ = device.config_calibration_set_matrix(calibration_matrix);
        }
        if let Some(area_rectangle) = self.area_rectangle {
            let _ = device.config_area_set_rectangle(area_rectangle);
        }
        if let Some(click_method) = self.click_method {
            let _ = device.config_click_set_method(click_method);
        }
        if let Some(clickfinger_button_map) = self.clickfinger_button_map {
            let _ = device.config_click_clickfinger_set_button_map(clickfinger_button_map);
        }
        if let Some(dwt_enabled) = self.dwt_enabled {
            let _ = device.config_dwt_set_enabled(dwt_enabled);
        }
        if let Some(dwtp_enabled) = self.dwtp_enabled {
            let _ = device.config_dwtp_set_enabled(dwtp_enabled);
        }
        if let Some(left_handed) = self.left_handed {
            let _ = device.config_left_handed_set(left_handed);
        }
        if let Some(middle_emulation_enabled) = self.middle_emulation_enabled {
            let _ = device.config_middle_emulation_set_enabled(middle_emulation_enabled);
        }
        if let Some(rotation_angle) = self.rotation_angle {
            let _ = device.config_rotation_set_angle(rotation_angle);
        }
        if let Some(scroll_button) = self.scroll_button {
            let _ = device.config_scroll_set_button(scroll_button);
        }
        if let Some(scroll_method) = self.scroll_method {
            let _ = device.config_scroll_set_method(scroll_method);
        }
        if let Some(natural_scroll_enabled) = self.natural_scroll_enabled {
            let _ = device.config_scroll_set_natural_scroll_enabled(natural_scroll_enabled);
        }
        if let Some(scroll_button_lock) = self.scroll_button_lock {
            let _ = device.config_scroll_set_button_lock(scroll_button_lock);
        }
        if let Some(send_events_mode) = self.send_events_mode {
            let _ = device.config_send_events_set_mode(send_events_mode);
        }
        if let Some(tap_button_map) = self.tap_button_map {
            let _ = device.config_tap_set_button_map(tap_button_map);
        }
        if let Some(tap_drag_enabled) = self.tap_drag_enabled {
            let _ = device.config_tap_set_drag_enabled(tap_drag_enabled);
        }
        if let Some(tap_drag_lock_enabled) = self.tap_drag_lock_enabled {
            let _ = device.config_tap_set_drag_lock_enabled(tap_drag_lock_enabled);
        }
        if let Some(tap_enabled) = self.tap_enabled {
            let _ = device.config_tap_set_enabled(tap_enabled);
        }
        if let Some(threefg_drag_enabled) = self.threefg_drag_enabled {
            let _ = device.config_3fg_drag_set_enabled(threefg_drag_enabled);
        }
    }
}
