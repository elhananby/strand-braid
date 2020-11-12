/// This crate defines a trait, `AsyncCamera`, whose `frames` method returns a
/// `futures::Stream` for asynchronous usage.
///
/// It also provides a struct, `ThreadedAsyncCameraModule` which will take a
/// camera module implementing the `ci2::CameraModule` trait and wrap it into a
/// new struct that also implements the `ci2::CameraModule` in addition to
/// returning a `ThreadedAsyncCamera` which implements the `AsyncCamera` trait.
///
/// For `ThreadedAsyncCameraModule` to work, it requires that the wrapped camera
/// type `C` implements the `ci2::Camera` and `Send` traits. It operates by
/// serializing access to the camera by wrapping `Arc<Mutex<C>>`. The `frames`
/// method spawns a thread on on which an infinite loop is used to grab frames
/// from the camera. Therefore other camera access happens only between acquired
/// frames. When image exposure times are on the order of 10 msec, this means
/// calls to access the camera may take 10 msec.
///
/// The structs `ThreadedAsyncCameraModule` and `ThreadedAsyncCamera` here are a
/// generic implementation that can be used at the cost of spawning a new
/// thread.
///
/// It would be possible for an upstream camera backend module to directly
/// implement the `AsyncCamera` trait. Such a camera-specific backend could
/// implement `AsyncCamera` without serializing access to the camera
/// but rather by taking advantage of functionality in most camera drivers.

#[macro_use]
extern crate log;

use futures::stream::Stream;

use machine_vision_formats as formats;

use ci2::Result;
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Debug)]
pub enum FrameResult<T> {
    Frame(T),
    SingleFrameError(String),
}

pub trait AsyncCamera {
    type FrameType: timestamped_frame::FrameTrait + Send;
    /// asynchronous frame acquisition, get an infinite stream of frames
    fn frames<F>(
        &mut self,
        bufsize: usize,
        on_start: F,
    ) -> Result<Box<dyn Stream<Item = FrameResult<Self::FrameType>> + Send + Unpin>>
    where
        F: Fn() + Send + 'static;
}

pub struct ThreadedAsyncCamera<C, T> {
    camera: Arc<Mutex<C>>,
    frame_type: std::marker::PhantomData<T>,
    name: String,
    serial: String,
    model: String,
    vendor: String,
    /// When acquiring, has value of Some, else None.
    control_and_join_handle: Option<(thread_control::Control, std::thread::JoinHandle<()>)>,
}

fn _test_camera_is_send() {
    // Compile-time test to ensure WrappedCamera implements Send trait.
    fn implements<T: Send>() {}
    implements::<ThreadedAsyncCamera<i8, u8>>();
}

pub struct ThreadedAsyncCameraModule<M, C, T> {
    cam_module: M,
    name: String,
    camera_type: std::marker::PhantomData<C>,
    frame_type: std::marker::PhantomData<T>,
}

impl<C: 'static, T: 'static> ThreadedAsyncCamera<C, T>
where
    C: ci2::Camera<FrameType = T> + Send,
    T: timestamped_frame::FrameTrait + Send,
    Vec<u8>: From<T>,
{
    pub fn control_and_join_handle(
        self,
    ) -> Option<(thread_control::Control, std::thread::JoinHandle<()>)> {
        self.control_and_join_handle
    }
}

impl<C: 'static, T: 'static> AsyncCamera for ThreadedAsyncCamera<C, T>
where
    C: ci2::Camera<FrameType = T> + Send,
    T: timestamped_frame::FrameTrait + Send,
    Vec<u8>: From<T>,
{
    type FrameType = T;

    fn frames<F>(
        &mut self,
        bufsize: usize,
        on_start: F,
    ) -> Result<Box<dyn Stream<Item = FrameResult<T>> + Send + Unpin>>
    where
        F: Fn() + Send + 'static,
    {
        if self.control_and_join_handle.is_some() {
            return Err(ci2::Error::CI2Error("already launched thread".to_string()));
        }

        let (mut tx, rx) = futures::channel::mpsc::channel(bufsize);

        let (flag, control) = thread_control::make_pair();

        let thread_builder =
            std::thread::Builder::new().name(format!("ThreadedAsyncCamera-{}", self.name));
        let cam_arc = self.camera.clone();
        let join_handle: std::thread::JoinHandle<()> = thread_builder.spawn(move || {
            on_start();
            while flag.is_alive() {
                // We need to release and re-acquire the lock every cycle to
                // allow other threads the chance to grab the lock.
                {
                    let mut cam = cam_arc.lock();
                    let msg = match cam.next_frame() {
                        Ok(frame) => FrameResult::Frame(frame),
                        Err(ci2::Error::SingleFrameError(s)) => FrameResult::SingleFrameError(s),
                        Err(e) => {
                            error!(
                                "fatal error acquiring frames: {} {:?} {}:{}",
                                e,
                                e,
                                file!(),
                                line!()
                            );
                            return;
                        }
                    };

                    match tx.try_send(msg) {
                        Ok(()) => {} // message put in channel ok
                        Err(e) => {
                            if e.is_full() {
                                // channel was full
                                error!("dropping message due to backpressure");
                            }
                            if e.is_disconnected() {
                                debug!("ThreadedAsyncCamera listener disconnected");
                                return;
                            }
                        }
                    };
                }
            }
            debug!(
                "closing thread {:?} ({:?}) in {}:{}",
                std::thread::current().name(),
                std::thread::current().id(),
                file!(),
                line!()
            );
        })?;

        self.control_and_join_handle = Some((control, join_handle));

        Ok(Box::new(rx))
    }
}

impl<M, C, T> ThreadedAsyncCameraModule<M, C, T>
where
    M: ci2::CameraModule<FrameType = T, CameraType = C>,
    C: ci2::Camera<FrameType = T>,
    T: timestamped_frame::FrameTrait,
    Vec<u8>: From<T>,
{
    pub fn threaded_async_camera(&mut self, name: &str) -> Result<ThreadedAsyncCamera<C, T>> {
        let camera = self.cam_module.camera(name)?;
        let name = camera.name().into();
        let model = camera.name().into();
        let serial = camera.serial().into();
        let vendor = camera.vendor().into();

        Ok(ThreadedAsyncCamera {
            camera: Arc::new(Mutex::new(camera)),
            frame_type: std::marker::PhantomData,
            name,
            model,
            vendor,
            serial,
            control_and_join_handle: None,
        })
    }
}

pub fn into_threaded_async<M, C, T>(cam_module: M) -> ThreadedAsyncCameraModule<M, C, T>
where
    M: ci2::CameraModule<FrameType = T, CameraType = C>,
    C: ci2::Camera<FrameType = T>,
    T: timestamped_frame::FrameTrait,
    Vec<u8>: From<T>,
{
    let name = format!("async-{}", cam_module.name());
    ThreadedAsyncCameraModule {
        cam_module,
        name,
        camera_type: std::marker::PhantomData,
        frame_type: std::marker::PhantomData,
    }
}

// ----

impl<C, T> ci2::CameraInfo for ThreadedAsyncCamera<C, T>
where
    C: ci2::CameraInfo,
{
    fn name(&self) -> &str {
        &self.name
    }
    fn serial(&self) -> &str {
        &self.serial
    }
    fn model(&self) -> &str {
        &self.model
    }
    fn vendor(&self) -> &str {
        &self.vendor
    }
}

impl<C, T> ci2::Camera for ThreadedAsyncCamera<C, T>
where
    C: ci2::Camera<FrameType = T>,
    T: timestamped_frame::FrameTrait,
    Vec<u8>: From<T>,
{
    type FrameType = T;

    fn width(&self) -> ci2::Result<u32> {
        let c = self.camera.lock();
        c.width()
    }
    fn height(&self) -> ci2::Result<u32> {
        let c = self.camera.lock();
        c.height()
    }
    fn pixel_format(&self) -> ci2::Result<formats::PixelFormat> {
        let c = self.camera.lock();
        c.pixel_format()
    }
    fn possible_pixel_formats(&self) -> ci2::Result<Vec<formats::PixelFormat>> {
        let c = self.camera.lock();
        c.possible_pixel_formats()
    }
    fn set_pixel_format(&mut self, pixel_format: formats::PixelFormat) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_pixel_format(pixel_format)
    }
    fn exposure_time(&self) -> ci2::Result<f64> {
        let c = self.camera.lock();
        c.exposure_time()
    }
    fn exposure_time_range(&self) -> ci2::Result<(f64, f64)> {
        let c = self.camera.lock();
        c.exposure_time_range()
    }
    fn set_exposure_time(&mut self, value: f64) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_exposure_time(value)
    }
    fn gain(&self) -> ci2::Result<f64> {
        let c = self.camera.lock();
        c.gain()
    }
    fn gain_range(&self) -> ci2::Result<(f64, f64)> {
        let c = self.camera.lock();
        c.gain_range()
    }
    fn set_gain(&mut self, value: f64) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_gain(value)
    }
    fn exposure_auto(&self) -> ci2::Result<ci2::AutoMode> {
        let c = self.camera.lock();
        c.exposure_auto()
    }
    fn set_exposure_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_exposure_auto(value)
    }
    fn gain_auto(&self) -> ci2::Result<ci2::AutoMode> {
        let c = self.camera.lock();
        c.gain_auto()
    }
    fn set_gain_auto(&mut self, value: ci2::AutoMode) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_gain_auto(value)
    }

    fn trigger_mode(&self) -> ci2::Result<ci2::TriggerMode> {
        let c = self.camera.lock();
        c.trigger_mode()
    }
    fn set_trigger_mode(&mut self, value: ci2::TriggerMode) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_trigger_mode(value)
    }

    fn acquisition_frame_rate_enable(&self) -> ci2::Result<bool> {
        let c = self.camera.lock();
        c.acquisition_frame_rate_enable()
    }
    fn set_acquisition_frame_rate_enable(&mut self, value: bool) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_acquisition_frame_rate_enable(value)
    }

    fn acquisition_frame_rate(&self) -> ci2::Result<f64> {
        let c = self.camera.lock();
        c.acquisition_frame_rate()
    }
    fn acquisition_frame_rate_range(&self) -> ci2::Result<(f64, f64)> {
        let c = self.camera.lock();
        c.acquisition_frame_rate_range()
    }
    fn set_acquisition_frame_rate(&mut self, value: f64) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_acquisition_frame_rate(value)
    }

    fn trigger_selector(&self) -> ci2::Result<ci2::TriggerSelector> {
        let c = self.camera.lock();
        c.trigger_selector()
    }
    fn set_trigger_selector(&mut self, value: ci2::TriggerSelector) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_trigger_selector(value)
    }

    fn acquisition_mode(&self) -> ci2::Result<ci2::AcquisitionMode> {
        let c = self.camera.lock();
        c.acquisition_mode()
    }
    fn set_acquisition_mode(&mut self, value: ci2::AcquisitionMode) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.set_acquisition_mode(value)
    }

    fn acquisition_start(&mut self) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.acquisition_start()
    }
    fn acquisition_stop(&mut self) -> ci2::Result<()> {
        let mut c = self.camera.lock();
        c.acquisition_stop()
    }

    /// blocks forever.
    fn next_frame(&mut self) -> ci2::Result<Self::FrameType> {
        let mut c = self.camera.lock();
        c.next_frame()
    }
}

impl<M, C, T> ci2::CameraModule for ThreadedAsyncCameraModule<M, C, T>
where
    M: ci2::CameraModule<FrameType = T, CameraType = C>,
    C: ci2::Camera<FrameType = T>,
    T: timestamped_frame::FrameTrait,
    Vec<u8>: From<T>,
{
    type FrameType = T;
    type CameraType = C;

    fn name(&self) -> &str {
        self.name.as_ref()
    }
    fn camera_infos(&self) -> Result<Vec<Box<dyn ci2::CameraInfo>>> {
        self.cam_module.camera_infos()
    }
    fn camera(&mut self, name: &str) -> Result<C> {
        self.cam_module.camera(name)
    }
}
