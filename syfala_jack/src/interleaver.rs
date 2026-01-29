use core::{iter, mem, num, ptr};

// One might argue this is a bit hacky

/// Allows interleaving samples, from a set of jack ports,
/// but allocates space for the pointers only once.
/// (To avoid allocating in RT threads)
#[repr(transparent)]
pub(crate) struct Interleaver<Spec> {
    ptrs: [(jack::Port<Spec>, ptr::NonNull<f32>)],
}

// TODO: What's the safety argument here?
unsafe impl<Spec> Send for Interleaver<Spec> {}

impl<Spec> Interleaver<Spec> {
    #[inline(always)]
    pub(crate) fn new(ports: impl IntoIterator<Item = jack::Port<Spec>>) -> Option<Box<Self>> {
        let boxed_slice = Box::from_iter(iter::zip(
            ports,
            iter::repeat_with(ptr::NonNull::<f32>::dangling),
        ));

        // check if the length is valid
        let _len = num::NonZeroU32::new(boxed_slice.len().try_into().ok()?)?;

        // SAFETY: We are a `#[repr(transparent)]` struct
        Some(unsafe { mem::transmute(boxed_slice) })
    }

    #[inline(always)]
    pub(crate) fn n_ports(&self) -> num::NonZeroU32 {
        // we return none when we create an interleaver with a channel count of 0
        // or when it's length exceeds u32::MAX
        num::NonZeroU32::new(self.ptrs.len().try_into().unwrap()).unwrap()
    }
}

// See this: (https://predr.ag/blog/definitive-guide-to-sealed-traits-in-rust/)
mod private {
    pub(crate) trait Sealed {}
    impl Sealed for jack::AudioIn {}
    impl Sealed for jack::AudioOut {}
}

pub(crate) trait ToJackPointer: private::Sealed {
    fn to_jack_buf_ptr(
        port: &mut jack::Port<Self>,
        scope: &jack::ProcessScope,
    ) -> ptr::NonNull<f32>
    where
        Self: Sized;
}

impl ToJackPointer for jack::AudioIn {
    #[inline(always)]
    fn to_jack_buf_ptr(port: &mut jack::Port<Self>, scope: &jack::ProcessScope) -> ptr::NonNull<f32>
    where
        Self: Sized,
    {
        ptr::NonNull::new(port.as_slice(scope).as_ptr().cast_mut()).unwrap()
    }
}

impl ToJackPointer for jack::AudioOut {
    #[inline(always)]
    fn to_jack_buf_ptr(port: &mut jack::Port<Self>, scope: &jack::ProcessScope) -> ptr::NonNull<f32>
    where
        Self: Sized,
    {
        ptr::NonNull::new(port.as_mut_slice(scope).as_ptr().cast_mut()).unwrap()
    }
}

pub(crate) trait FromJackPointer: private::Sealed {
    type Output<'a>;
    unsafe fn get_ref<'a>(ptr: ptr::NonNull<f32>) -> Self::Output<'a>;
}

impl FromJackPointer for jack::AudioIn {
    type Output<'a> = &'a f32;

    #[inline(always)]
    unsafe fn get_ref<'a>(ptr: ptr::NonNull<f32>) -> Self::Output<'a> {
        // SAFETY: ensured by the caller
        unsafe { ptr.as_ref() }
    }
}

impl FromJackPointer for jack::AudioOut {
    type Output<'a> = &'a mut f32;

    #[inline(always)]
    unsafe fn get_ref<'a>(mut ptr: ptr::NonNull<f32>) -> Self::Output<'a> {
        // SAFETY: ensured by the caller
        unsafe { ptr.as_mut() }
    }
}

impl<Spec: FromJackPointer + ToJackPointer> Interleaver<Spec> {
    #[inline(always)]
    pub(crate) fn interleave(
        &mut self,
        process_scope: &jack::ProcessScope,
    ) -> impl ExactSizeIterator<Item = Spec::Output<'_>> {
        // Write the pointers into our list

        for (port, ptr) in &mut self.ptrs.iter_mut() {
            *ptr = Spec::to_jack_buf_ptr(port, process_scope);
        }

        // Then return the iterator

        Interleaved {
            remaining_frames: usize::try_from(process_scope.n_frames()).unwrap(),
            current_index: 0,
            ptrs: &mut self.ptrs,
        }
    }
}

pub(crate) struct Interleaved<'a, Spec> {
    remaining_frames: usize,
    current_index: usize,
    ptrs: &'a mut [(jack::Port<Spec>, ptr::NonNull<f32>)],
}

impl<'a, Spec: FromJackPointer> Iterator for Interleaved<'a, Spec> {
    type Item = Spec::Output<'a>;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining_frames == 0 {
            return None;
        }

        // SAFETY: current_idx starts at 0 and wraps around at ptrs.len
        // + ptrs.len() != 0
        let (_port, ptr_ref) = unsafe { self.ptrs.get_unchecked_mut(self.current_index) };
        let ptr = *ptr_ref;
        // SAFETY: happens at most remaining_frames times
        // ensuring we're within the buffer's bounds
        *ptr_ref = unsafe { ptr_ref.add(1) };
        // never panics, is always less or equal to self.ptrs.len()
        self.current_index = self.current_index.strict_add(1);
        if self.current_index == self.ptrs.len() {
            self.current_index = 0;
            // never panics, we just checked that self.remaining_frames != 0
            self.remaining_frames = self.remaining_frames.strict_sub(1);
        }
        // SAFETY: we point to a valid JACK buffer pointer
        Some(unsafe { Spec::get_ref(ptr) })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.remaining_frames * self.ptrs.len();
        (len, Some(len))
    }
}

impl<'a, Spec: FromJackPointer> ExactSizeIterator for Interleaved<'a, Spec> {
    fn len(&self) -> usize {
        self.size_hint().0
    }
}
