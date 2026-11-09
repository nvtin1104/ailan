//! Critically-damped-ish spring used for the notch size and fades.

#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        Self { value, target: value, velocity: 0.0, stiffness: 420.0, damping: 34.0 }
    }

    pub fn with(mut self, stiffness: f32, damping: f32) -> Self {
        self.stiffness = stiffness;
        self.damping = damping;
        self
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Jump straight to the target (used when animations are disabled).
    pub fn snap(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }

    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < 0.05 && self.velocity.abs() < 0.05
    }

    /// Advance by `dt` seconds. Returns `true` once settled (and snaps).
    pub fn step(&mut self, dt: f32) -> bool {
        // Sub-step for stability when a frame is late.
        let mut remaining = dt.clamp(0.0, 0.1);
        while remaining > 0.0 {
            let h = remaining.min(1.0 / 240.0);
            let force = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
            self.velocity += force * h;
            self.value += self.velocity * h;
            remaining -= h;
        }
        if self.is_settled() {
            self.snap();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_to_target() {
        let mut s = Spring::new(0.0);
        s.set_target(100.0);
        let mut frames = 0;
        while !s.step(1.0 / 60.0) {
            frames += 1;
            assert!(frames < 240, "spring did not settle");
        }
        assert_eq!(s.value, 100.0);
        assert!(frames > 5, "spring settled implausibly fast");
    }

    #[test]
    fn settled_spring_stays_settled() {
        let mut s = Spring::new(5.0);
        assert!(s.step(1.0 / 60.0));
        assert_eq!(s.value, 5.0);
    }
}
