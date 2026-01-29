//! Video Decoder for Screen Share
//!
//! Uses GStreamer to decode H.264 video streams from Android devices.

#[cfg(feature = "screenshare")]
use gstreamer as gst;
#[cfg(feature = "screenshare")]
use gstreamer::prelude::*;
#[cfg(feature = "screenshare")]
use gstreamer_app as gst_app;
#[cfg(feature = "screenshare")]
use crate::Result;
#[cfg(feature = "screenshare")]
use tracing::{debug, error};

/// Video decoder
#[cfg(feature = "screenshare")]
pub struct VideoDecoder {
    pipeline: gst::Pipeline,
    appsrc: gst_app::AppSrc,
    appsink: gst_app::AppSink,
}

#[cfg(feature = "screenshare")]
impl VideoDecoder {
    /// Create a new video decoder
    pub fn new() -> Result<Self> {
        gst::init().map_err(|e| crate::ProtocolError::Plugin(format!("GStreamer init failed: {}", e)))?;

        // Create pipeline: appsrc -> h264parse -> avdec_h264 -> videoconvert -> appsink
        // Using raw H.264 stream
        let pipeline_str = "appsrc name=src format=time is-live=true do-timestamp=true ! h264parse ! avdec_h264 ! videoconvert ! video/x-raw,format=RGBA ! appsink name=sink drop=true max-buffers=1";
        
        debug!("Creating GStreamer pipeline: {}", pipeline_str);
        
        let pipeline = gst::parse::launch(pipeline_str)
            .map_err(|e| crate::ProtocolError::Plugin(format!("Failed to parse pipeline: {}", e)))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| crate::ProtocolError::Plugin("Failed to downcast pipeline".to_string()))?;

        let appsrc = pipeline
            .by_name("src")
            .ok_or_else(|| crate::ProtocolError::Plugin("Failed to get appsrc".to_string()))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| crate::ProtocolError::Plugin("Failed to downcast appsrc".to_string()))?;

        let appsink = pipeline
            .by_name("sink")
            .ok_or_else(|| crate::ProtocolError::Plugin("Failed to get appsink".to_string()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| crate::ProtocolError::Plugin("Failed to downcast appsink".to_string()))?;

        // Configure caps for appsrc (optional but good for negotiation)
        // appsrc.set_caps(Some(&gst::Caps::builder("video/x-h264").build()));

        Ok(Self {
            pipeline,
            appsrc,
            appsink,
        })
    }

    /// Start the decoder
    pub fn start(&self) -> Result<()> {
                self.pipeline.set_state(gst::State::Playing)
            .map_err(|e| crate::ProtocolError::Plugin(format!("Failed to start pipeline: {}", e)))?;
        debug!("Video decoder started");
        Ok(())
    }

    /// Stop the decoder
    pub fn stop(&self) -> Result<()> {
                self.pipeline.set_state(gst::State::Null)
            .map_err(|e| crate::ProtocolError::Plugin(format!("Failed to stop pipeline: {}", e)))?;
        debug!("Video decoder stopped");
        Ok(())
    }

    /// Push encoded frame data (H.264 NAL unit)
    pub fn push_frame(&self, data: &[u8]) -> Result<()> {
                
        // Create buffer
        let buffer = gst::Buffer::from_slice(data.to_vec()); // Copying for now
        
        // Push to appsrc
        if let Err(e) = self.appsrc.push_buffer(buffer) {
             error!("Failed to push buffer to GStreamer: {}", e);
             return Err(crate::ProtocolError::Plugin(format!("Failed to push buffer: {}", e)));
        }
            
        Ok(())
    }
    
    /// Pull decoded frame (RGBA)
    pub fn pull_frame(&self) -> Result<Option<(Vec<u8>, u32, u32)>> {
                
        // Try to pull a sample with a small timeout to avoid blocking
        match self.appsink.try_pull_sample(gst::ClockTime::from_mseconds(5)) {
            Some(sample) => {
                let caps = sample.caps().ok_or_else(|| crate::ProtocolError::Plugin("No caps in sample".to_string()))?;
                let structure = caps.structure(0).ok_or_else(|| crate::ProtocolError::Plugin("No structure in caps".to_string()))?;
                
                let width = structure.get::<i32>("width").map_err(|_| crate::ProtocolError::Plugin("No width in caps".to_string()))? as u32;
                let height = structure.get::<i32>("height").map_err(|_| crate::ProtocolError::Plugin("No height in caps".to_string()))? as u32;
                
                let buffer = sample.buffer().ok_or_else(|| crate::ProtocolError::Plugin("No buffer in sample".to_string()))?;
                let map = buffer.map_readable().map_err(|_| crate::ProtocolError::Plugin("Failed to map buffer".to_string()))?;
                
                Ok(Some((map.to_vec(), width, height)))
            },
            None => Ok(None),
        }
    }
}

// Stub implementation for when screenshare feature is disabled
#[cfg(not(feature = "screenshare"))]
pub struct VideoDecoder;

#[cfg(not(feature = "screenshare"))]
impl VideoDecoder {
    pub fn new() -> crate::Result<Self> {
        Ok(Self)
    }
    
    pub fn start(&self) -> crate::Result<()> {
        Ok(())
    }
    
    pub fn stop(&self) -> crate::Result<()> {
        Ok(())
    }
    
    pub fn push_frame(&self, _data: &[u8]) -> crate::Result<()> {
        Ok(())
    }
    
    pub fn pull_frame(&self) -> crate::Result<Option<(Vec<u8>, u32, u32)>> {
        Ok(None)
    }
}
