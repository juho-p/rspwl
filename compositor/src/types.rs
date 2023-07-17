// dumb stuff but we don't have much of this

pub type Result<Res> = std::result::Result<Res, String>;

pub type NodeId = u32;

#[derive(Debug, Clone)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Default for Rect {
    fn default() -> Self {
        Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        }
    }
}
