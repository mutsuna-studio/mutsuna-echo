//! Minimal online stream types shared by offline ASR and speaker embeddings.

use crate::utils::to_c_ptr;
use sherpa_onnx_sys as sys;
use std::{ffi::CString, ptr};

#[derive(Clone, Debug, Default)]
pub struct HomophoneReplacerConfig {
    pub lexicon: Option<String>,
    pub rule_fsts: Option<String>,
}

impl HomophoneReplacerConfig {
    pub(crate) fn to_sys(&self, cstrings: &mut Vec<CString>) -> sys::HomophoneReplacerConfig {
        sys::HomophoneReplacerConfig {
            dict_dir: ptr::null(),
            lexicon: to_c_ptr(&self.lexicon, cstrings),
            rule_fsts: to_c_ptr(&self.rule_fsts, cstrings),
        }
    }
}

pub struct OnlineStream {
    pub(crate) ptr: *const sys::OnlineStream,
}

unsafe impl Send for OnlineStream {}
unsafe impl Sync for OnlineStream {}

impl OnlineStream {
    pub fn accept_waveform(&self, sample_rate: i32, samples: &[f32]) {
        unsafe {
            sys::SherpaOnnxOnlineStreamAcceptWaveform(
                self.ptr,
                sample_rate,
                samples.as_ptr(),
                samples.len() as i32,
            )
        }
    }

    pub fn input_finished(&self) {
        unsafe { sys::SherpaOnnxOnlineStreamInputFinished(self.ptr) }
    }
}

impl Drop for OnlineStream {
    fn drop(&mut self) {
        unsafe { sys::SherpaOnnxDestroyOnlineStream(self.ptr) }
    }
}
