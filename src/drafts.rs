impl TryFrom<i32> for Drink {
    type Error = &'static str;

    fn try_from(x: i32) -> Result<Self, Self::Error> {
        match x {
            0..=2 => Ok(unsafe { std::mem::transmute(x as u8) }),
            _ => Err("invalid Drink value"),
        }
    }
}
