// Copyright 2017 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! An intermediate surface on the GPU used during the rasterization process.

use compute_shader::buffer::Protection;
use compute_shader::device::Device;
use compute_shader::image::{ExternalImage, Format, Image};
use error::InitError;
use euclid::size::Size2D;
use gl::types::{GLint, GLuint};
use gl;

/// An intermediate surface on the GPU used during the rasterization process.
///
/// You can reuse this surface from draw operation to draw operation. It only needs to be at least
/// as large as every atlas you will draw into it.
///
/// The GPU memory usage of this buffer is `4 * width * height` bytes.
pub struct CoverageBuffer {
    image: Image,
    framebuffer: GLuint,
}

impl CoverageBuffer {
    /// Creates a new coverage buffer of the given size.
    ///
    /// The size must be at least as large as every atlas you will render with it.
    pub fn new(device: &Device, size: &Size2D<u32>) -> Result<CoverageBuffer, InitError> {
        let image = try!(device.create_image(Format::R32F, Protection::ReadWrite, size)
                               .map_err(InitError::ComputeError));

        let mut framebuffer = 0;
        unsafe {
            let mut gl_texture = 0;
            gl::GenTextures(1, &mut gl_texture);
            try!(image.bind_to(&ExternalImage::GlTexture(gl_texture))
                      .map_err(InitError::ComputeError));

            gl::BindTexture(gl::TEXTURE_RECTANGLE, gl_texture);
            gl::TexParameteri(gl::TEXTURE_RECTANGLE, gl::TEXTURE_MIN_FILTER, gl::LINEAR as GLint);
            gl::TexParameteri(gl::TEXTURE_RECTANGLE, gl::TEXTURE_MAG_FILTER, gl::LINEAR as GLint);
            gl::TexParameteri(gl::TEXTURE_RECTANGLE,
                              gl::TEXTURE_WRAP_S,
                              gl::CLAMP_TO_EDGE as GLint);
            gl::TexParameteri(gl::TEXTURE_RECTANGLE,
                              gl::TEXTURE_WRAP_T,
                              gl::CLAMP_TO_EDGE as GLint);

            gl::GenFramebuffers(1, &mut framebuffer);
            gl::BindFramebuffer(gl::FRAMEBUFFER, framebuffer);
            gl::FramebufferTexture2D(gl::FRAMEBUFFER,
                                     gl::COLOR_ATTACHMENT0,
                                     gl::TEXTURE_RECTANGLE,
                                     gl_texture,
                                     0);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        }

        Ok(CoverageBuffer {
            image: image,
            framebuffer: framebuffer,
        })
    }

    #[doc(hidden)]
    #[inline]
    pub fn image(&self) -> &Image {
        &self.image
    }

    #[doc(hidden)]
    #[inline]
    pub fn framebuffer(&self) -> GLuint {
        self.framebuffer
    }
}

impl Drop for CoverageBuffer {
    fn drop(&mut self) {
        unsafe {
            let mut gl_texture = 0;
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.framebuffer);
            gl::GetFramebufferAttachmentParameteriv(gl::FRAMEBUFFER,
                                                    gl::COLOR_ATTACHMENT0,
                                                    gl::FRAMEBUFFER_ATTACHMENT_OBJECT_NAME,
                                                    &mut gl_texture as *mut GLuint as *mut GLint);
            gl::DeleteTextures(1, &mut gl_texture);

            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::DeleteFramebuffers(1, &mut self.framebuffer);
        }
    }
}

