// Copyright 2017 The Servo Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Glyph vectors, uploaded in a resolution-independent manner to the GPU.

use error::GlError;
use euclid::Size2D;
use gl::types::{GLsizeiptr, GLuint};
use gl;
use otf::{self, Font};
use std::mem;
use std::os::raw::c_void;

static DUMMY_VERTEX: Vertex = Vertex {
    x: 0,
    y: 0,
    glyph_index: 0,
};

/// Packs up outlines for glyphs into a format that the GPU can process.
pub struct OutlineBuilder {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    descriptors: Vec<GlyphDescriptor>,
}

impl OutlineBuilder {
    /// Creates a new empty set of outlines.
    #[inline]
    pub fn new() -> OutlineBuilder {
        OutlineBuilder {
            vertices: vec![DUMMY_VERTEX],
            indices: vec![],
            descriptors: vec![],
        }
    }

    /// Adds a new glyph to the outline builder. Returns the glyph index, which is useful for later
    /// calls to `Atlas::pack_glyph()`.
    pub fn add_glyph(&mut self, font: &Font, glyph_id: u16) -> Result<u16, otf::Error> {
        let glyph_index = self.descriptors.len() as u16;

        let mut point_index = self.vertices.len() as u32;
        let start_index = self.indices.len() as u32;
        let start_point = point_index;
        let mut last_point_on_curve = true;

        try!(font.for_each_point(glyph_id, |point| {
            self.vertices.push(Vertex {
                x: point.position.x,
                y: point.position.y,
                glyph_index: glyph_index,
            });

            if point.index_in_contour > 0 && point.on_curve {
                let indices = if !last_point_on_curve {
                    [point_index - 2, point_index - 1, point_index]
                } else {
                    [point_index - 1, 0, point_index]
                };
                self.indices.extend(indices.iter().cloned());
            }

            point_index += 1;
            last_point_on_curve = point.on_curve
        }));

        // Add a glyph descriptor.
        self.descriptors.push(GlyphDescriptor {
            bounds: try!(font.glyph_bounds(glyph_id)),
            units_per_em: font.units_per_em() as u32,
            start_point: start_point as u32,
            start_index: start_index,
            glyph_id: glyph_id,
        });

        Ok(glyph_index)
    }

    /// Uploads the outlines to the GPU.
    pub fn create_buffers(self) -> Result<Outlines, GlError> {
        // TODO(pcwalton): Try using `glMapBuffer` here. Requires precomputing contour types and
        // counts.
        unsafe {
            let (mut vertices, mut indices, mut descriptors) = (0, 0, 0);
            gl::GenBuffers(1, &mut vertices);
            gl::GenBuffers(1, &mut indices);
            gl::GenBuffers(1, &mut descriptors);

            gl::BindBuffer(gl::ARRAY_BUFFER, vertices);
            gl::BufferData(gl::ARRAY_BUFFER,
                           (self.vertices.len() * mem::size_of::<Vertex>()) as GLsizeiptr,
                           self.vertices.as_ptr() as *const Vertex as *const c_void,
                           gl::STATIC_DRAW);

            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, indices);
            gl::BufferData(gl::ELEMENT_ARRAY_BUFFER,
                           (self.indices.len() * mem::size_of::<u32>()) as GLsizeiptr,
                           self.indices.as_ptr() as *const u32 as *const c_void,
                           gl::STATIC_DRAW);

            let length = self.descriptors.len() * mem::size_of::<GlyphDescriptor>();
            gl::BindBuffer(gl::UNIFORM_BUFFER, descriptors);
            gl::BufferData(gl::UNIFORM_BUFFER,
                           length as GLsizeiptr,
                           self.descriptors.as_ptr() as *const GlyphDescriptor as *const c_void,
                           gl::STATIC_DRAW);

            Ok(Outlines {
                vertices_buffer: vertices,
                indices_buffer: indices,
                descriptors_buffer: descriptors,
                descriptors: self.descriptors,
                indices_count: self.indices.len(),
            })
        }
    }
}

/// Resolution-independent glyph vectors uploaded to the GPU.
pub struct Outlines {
    vertices_buffer: GLuint,
    indices_buffer: GLuint,
    descriptors_buffer: GLuint,
    descriptors: Vec<GlyphDescriptor>,
    indices_count: usize,
}

impl Drop for Outlines {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteBuffers(1, &mut self.descriptors_buffer);
            gl::DeleteBuffers(1, &mut self.indices_buffer);
            gl::DeleteBuffers(1, &mut self.vertices_buffer);
        }
    }
}

impl Outlines {
    #[doc(hidden)]
    #[inline]
    pub fn vertices_buffer(&self) -> GLuint {
        self.vertices_buffer
    }

    #[doc(hidden)]
    #[inline]
    pub fn indices_buffer(&self) -> GLuint {
        self.indices_buffer
    }

    #[doc(hidden)]
    #[inline]
    pub fn descriptors_buffer(&self) -> GLuint {
        self.descriptors_buffer
    }

    #[doc(hidden)]
    #[inline]
    pub fn descriptor(&self, glyph_index: u16) -> Option<&GlyphDescriptor> {
        self.descriptors.get(glyph_index as usize)
    }

    #[doc(hidden)]
    #[inline]
    pub fn indices_count(&self) -> usize {
        self.indices_count
    }

    /// Returns the glyph rectangle in font units.
    #[inline]
    pub fn glyph_bounds(&self, glyph_index: u32) -> GlyphBounds {
        self.descriptors[glyph_index as usize].bounds
    }

    /// Returns the glyph rectangle in fractional pixels.
    #[inline]
    pub fn glyph_subpixel_bounds(&self, glyph_index: u16, point_size: f32) -> GlyphSubpixelBounds {
        self.descriptors[glyph_index as usize].subpixel_bounds(point_size)
    }

    /// Returns the boundaries of the glyph, rounded out to the nearest pixel.
    #[inline]
    pub fn glyph_pixel_bounds(&self, glyph_index: u16, point_size: f32) -> GlyphPixelBounds {
        self.descriptors[glyph_index as usize].subpixel_bounds(point_size).round_out()
    }

    /// Returns the ID of the glyph with the given index.
    #[inline]
    pub fn glyph_id(&self, glyph_index: u16) -> u16 {
        self.descriptors[glyph_index as usize].glyph_id
    }
}

#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GlyphDescriptor {
    bounds: GlyphBounds,
    units_per_em: u32,
    start_point: u32,
    start_index: u32,
    glyph_id: u16,
}

impl GlyphDescriptor {
    #[doc(hidden)]
    #[inline]
    pub fn start_index(&self) -> u32 {
        self.start_index
    }

    #[doc(hidden)]
    #[inline]
    fn subpixel_bounds(&self, point_size: f32) -> GlyphSubpixelBounds {
        self.bounds.subpixel_bounds(self.units_per_em as u16, point_size)
    }
}

#[doc(hidden)]
#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct Vertex {
    x: i16,
    y: i16,
    glyph_index: u16,
}

/// The boundaries of the glyph in fractional pixels.
#[derive(Copy, Clone, Debug)]
pub struct GlyphSubpixelBounds {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

impl GlyphSubpixelBounds {
    /// Rounds these bounds out to the nearest pixel.
    #[inline]
    pub fn round_out(&self) -> GlyphPixelBounds {
        GlyphPixelBounds {
            left: self.left.floor() as i32,
            bottom: self.bottom.floor() as i32,
            right: self.right.ceil() as i32,
            top: self.top.ceil() as i32,
        }
    }

    /// Returns the total size of the glyph in fractional pixels.
    #[inline]
    pub fn size(&self) -> Size2D<f32> {
        Size2D::new(self.right - self.left, self.top - self.bottom)
    }
}

/// The boundaries of the glyph, rounded out to the nearest pixel.
#[derive(Copy, Clone, Debug)]
pub struct GlyphPixelBounds {
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
    pub top: i32,
}

impl GlyphPixelBounds {
    /// Returns the total size of the glyph in whole pixels.
    #[inline]
    pub fn size(&self) -> Size2D<i32> {
        Size2D::new(self.right - self.left, self.top - self.bottom)
    }
}

/// The boundaries of a glyph in font units.
#[derive(Copy, Clone, Debug)]
pub struct GlyphBounds {
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
    pub top: i32,
}

impl GlyphBounds {
    /// Given the units per em of the font and the point size, returns the fractional boundaries of
    /// this glyph.
    #[inline]
    pub fn subpixel_bounds(&self, units_per_em: u16, point_size: f32) -> GlyphSubpixelBounds {
        let pixels_per_unit = point_size / units_per_em as f32;
        GlyphSubpixelBounds {
            left: self.left as f32 * pixels_per_unit,
            bottom: self.bottom as f32 * pixels_per_unit,
            right: self.right as f32 * pixels_per_unit,
            top: self.top as f32 * pixels_per_unit,
        }
    }

    /// Returns the total size of the glyph in font units.
    #[inline]
    pub fn size(&self) -> Size2D<i32> {
        Size2D::new(self.right - self.left, self.top - self.bottom)
    }
}

