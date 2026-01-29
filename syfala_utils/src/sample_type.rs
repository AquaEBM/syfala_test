use core::num;

pub trait SampleSize {
    const SIZE: num::NonZeroU8;
}

pub trait SampleFromBytes: SampleSize {
    
    /// # Panics
    ///
    /// if `slice.len() != Self::SIZE`
    // TODO: when/if NIGHTLY: #[feature(min_generic_const_args)] lands, make `slice` a
    // statically-sized array instead
    fn from_bytes(slice: &[u8]) -> Self;
}

pub trait SampleToBytes: SampleSize {
    /// # Panics
    ///
    /// if `slice.len() != Self::SIZE`
    // TODO: when/if NIGHTLY: #[feature(min_generic_const_args)] lands, return a
    // statically-sized array instead
    fn to_bytes(self, slice: &mut [u8]);
}

pub trait SampleTypeSilence {
    const SILENCE: Self;
}

// TODO: is it correct that the silence value for unsigned integers is the middle value?

impl SampleSize for u8 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(1).unwrap();
}

impl SampleFromBytes for u8 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for u8 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for u8 {
    const SILENCE: Self = Self::MAX / 2 + 1;
}

impl SampleSize for u16 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(2).unwrap();
}

impl SampleFromBytes for u16 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for u16 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for u16 {
    const SILENCE: Self = Self::MAX / 2 + 1;
}

// TODO: u24?

impl SampleSize for u32 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(4).unwrap();
}

impl SampleFromBytes for u32 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for u32 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for u32 {
    const SILENCE: Self = Self::MAX / 2 + 1;
}

impl SampleSize for u64 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(8).unwrap();
}

impl SampleFromBytes for u64 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for u64 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for u64 {
    const SILENCE: Self = Self::MAX / 2 + 1;
}

impl SampleSize for i8 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(1).unwrap();
}

impl SampleFromBytes for i8 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for i8 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for i8 {
    const SILENCE: Self = 0;
}

impl SampleSize for i16 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(2).unwrap();
}

impl SampleFromBytes for i16 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for i16 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for i16 {
    const SILENCE: Self = 0;
}

// TODO: i24?

impl SampleSize for i32 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(4).unwrap();
}

impl SampleFromBytes for i32 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for i32 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for i32 {
    const SILENCE: Self = 0;
}

impl SampleSize for i64 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(8).unwrap();
}

impl SampleFromBytes for i64 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for i64 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for i64 {
    const SILENCE: Self = 0;
}

impl SampleSize for f32 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(4).unwrap();
}

impl SampleFromBytes for f32 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for f32 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for f32 {
    const SILENCE: Self = 0.;
}

impl SampleSize for f64 {
    const SIZE: num::NonZeroU8 = num::NonZeroU8::new(8).unwrap();
}

impl SampleFromBytes for f64 {
    fn from_bytes(slice: &[u8]) -> Self {
        Self::from_le_bytes(slice.try_into().unwrap())
    }
}

impl SampleToBytes for f64 {
    fn to_bytes(self, slice: &mut [u8]) {
        *slice.as_mut_array().unwrap() = self.to_le_bytes();
    }
}

impl SampleTypeSilence for f64 {
    const SILENCE: Self = 0.;
}
