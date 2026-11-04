//! Updates through Sparkle. Only packaged releases carry the framework, so it is loaded at runtime
//! and other builds simply run without updates.
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use objc2_foundation::{NSBundle, NSString};

fn controller_class() -> Result<&'static AnyClass, String> {
    let frameworks = NSBundle::mainBundle().privateFrameworksPath().ok_or("no frameworks folder")?;
    let path = NSString::from_str(&format!("{frameworks}/Sparkle.framework"));
    let sparkle = NSBundle::bundleWithPath(&path).ok_or("no Sparkle.framework")?;
    unsafe { sparkle.loadAndReturnError() }.map_err(|e| e.to_string())?;
    AnyClass::get(c"SPUStandardUpdaterController").ok_or_else(|| "no updater controller".to_string())
}

/// Whether this build can load Sparkle, for the packaging checks.
pub fn check() -> Result<(), String> {
    controller_class().map(|_| ())
}

pub struct Updater {
    controller: Retained<AnyObject>,
}

impl Updater {
    /// Starts Sparkle, which asks the user about automatic checks on the second launch.
    pub fn start() -> Option<Updater> {
        let class = controller_class().ok()?;
        let nil: *const AnyObject = std::ptr::null();
        let controller: Option<Retained<AnyObject>> = unsafe {
            let new: Allocated<AnyObject> = msg_send![class, alloc];
            msg_send![new, initWithStartingUpdater: true, updaterDelegate: nil, userDriverDelegate: nil]
        };
        controller.map(|controller| Updater { controller })
    }

    pub fn check_now(&self) {
        let nil: *const AnyObject = std::ptr::null();
        let _: () = unsafe { msg_send![&*self.controller, checkForUpdates: nil] };
    }

    fn updater(&self) -> Retained<AnyObject> {
        unsafe { msg_send![&*self.controller, updater] }
    }

    pub fn checks_automatically(&self) -> bool {
        unsafe { msg_send![&*self.updater(), automaticallyChecksForUpdates] }
    }

    pub fn set_checks_automatically(&self, on: bool) {
        let _: () = unsafe { msg_send![&*self.updater(), setAutomaticallyChecksForUpdates: on] };
    }
}
