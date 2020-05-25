use std::sync::Arc;
use crate::Fft;
pub(crate) use avx_vector::AvxVector256;

// Data that most (non-butterfly) SIMD FFT algorithms share
// Algorithms aren't required to use this struct, but it allows for a lot of reduction in code duplication
struct CommonSimdData<T, V> {
    inner_fft: Arc<dyn Fft<T>>,
    twiddles: Box<[V]>,

    len: usize,

    inplace_scratch_len: usize,
    outofplace_scratch_len: usize,

    inverse: bool,
}

macro_rules! boilerplate_fft_commondata {
    ($struct_name:ident) => (
        impl<T: FFTnum> $struct_name<T> {
            /// Preallocates necessary arrays and precomputes necessary data to efficiently compute the FFT
            /// Returns Ok() if this machine has the required instruction sets, Err() if some instruction sets are missing
            #[inline]
            pub fn new(inner_fft: Arc<dyn Fft<T>>) -> Result<Self, ()> {
                let has_avx = is_x86_feature_detected!("avx");
                let has_fma = is_x86_feature_detected!("fma");
                if has_avx && has_fma {
                    // Safety: new_with_avx requires the "avx" feature set. Since we know it's present, we're safe
                    Ok(unsafe { Self::new_with_avx(inner_fft) })
                } else {
                    Err(())
                }
            }
            #[inline]
            fn perform_fft_inplace(&self, buffer: &mut [Complex<T>], scratch: &mut [Complex<T>]) {
                // Perform the column FFTs
                // Safety: self.perform_column_butterflies() requres the "avx" and "fma" instruction sets, and we return Err() in our constructor if the instructions aren't available
                unsafe { self.perform_column_butterflies(buffer) };

                // process the row FFTs
                let (scratch, inner_scratch) = scratch.split_at_mut(self.len());
                self.common_data.inner_fft.process_multi(buffer, scratch, inner_scratch);

                // Transpose
                // Safety: self.transpose() requres the "avx" instruction set, and we return Err() in our constructor if the instructions aren't available
                unsafe { self.transpose(scratch, buffer) };
            }

            #[inline]
            fn perform_fft_out_of_place(&self, input: &mut [Complex<T>], output: &mut [Complex<T>], scratch: &mut [Complex<T>]) {
                // Perform the column FFTs
                // Safety: self.perform_column_butterflies() requres the "avx" and "fma" instruction sets, and we return Err() in our constructor if the instructions aren't avaiable
                unsafe { self.perform_column_butterflies(input) };

                // process the row FFTs. If extra scratch was provided, pass it in. Otherwise, use the output.
                let inner_scratch = if scratch.len() > 0 { scratch } else { &mut output[..] };
                self.common_data.inner_fft.process_inplace_multi(input, inner_scratch);

                // Transpose
                // Safety: self.transpose() requres the "avx" instruction set, and we return Err() in our constructor if the instructions aren't available
                unsafe { self.transpose(input, output) };
            }
        }

		impl<T: FFTnum> Fft<T> for $struct_name<T> {
            fn process_with_scratch(&self, input: &mut [Complex<T>], output: &mut [Complex<T>], scratch: &mut [Complex<T>]) {
                assert_eq!(input.len(), self.len(), "Input is the wrong length. Expected {}, got {}", self.len(), input.len());
                assert_eq!(output.len(), self.len(), "Output is the wrong length. Expected {}, got {}", self.len(), output.len());
                
                let required_scratch = self.get_out_of_place_scratch_len();
                assert!(scratch.len() >= required_scratch, "Scratch is the wrong length. Expected {} or greater, got {}", required_scratch, scratch.len());
        
                let scratch = &mut scratch[..required_scratch];
		
				self.perform_fft_out_of_place(input, output, scratch);
            }
            fn process_multi(&self, input: &mut [Complex<T>], output: &mut [Complex<T>], scratch: &mut [Complex<T>]) {
                assert!(input.len() % self.len() == 0, "Output is the wrong length. Expected multiple of {}, got {}", self.len(), input.len());
                assert_eq!(input.len(), output.len(), "Output is the wrong length. input = {} output = {}", input.len(), output.len());
                
                let required_scratch = self.get_out_of_place_scratch_len();
                assert!(scratch.len() >= required_scratch, "Scratch is the wrong length. Expected {} or greater, got {}", required_scratch, scratch.len());
        
                let scratch = &mut scratch[..required_scratch];
		
				for (in_chunk, out_chunk) in input.chunks_exact_mut(self.len()).zip(output.chunks_exact_mut(self.len())) {
					self.perform_fft_out_of_place(in_chunk, out_chunk, scratch);
				}
            }
            fn process_inplace_with_scratch(&self, buffer: &mut [Complex<T>], scratch: &mut [Complex<T>]) {
                assert_eq!(buffer.len(), self.len(), "Buffer is the wrong length. Expected {}, got {}", self.len(), buffer.len());

                let required_scratch = self.get_inplace_scratch_len();
                assert!(scratch.len() >= required_scratch, "Scratch is the wrong length. Expected {} or greater, got {}", required_scratch, scratch.len());
        
                let scratch = &mut scratch[..required_scratch];
        
                self.perform_fft_inplace(buffer, scratch);
            }
            fn process_inplace_multi(&self, buffer: &mut [Complex<T>], scratch: &mut [Complex<T>]) {
                assert_eq!(buffer.len() % self.len(), 0, "Buffer is the wrong length. Expected multiple of {}, got {}", self.len(), buffer.len());

                let required_scratch = self.get_inplace_scratch_len();
                assert!(scratch.len() >= required_scratch, "Scratch is the wrong length. Expected {} or greater, got {}", required_scratch, scratch.len());
        
                let scratch = &mut scratch[..required_scratch];
        
                for chunk in buffer.chunks_exact_mut(self.len()) {
                    self.perform_fft_inplace(chunk, scratch);
                }
            }
            #[inline(always)]
            fn get_inplace_scratch_len(&self) -> usize {
                self.common_data.inplace_scratch_len
            }
            #[inline(always)]
            fn get_out_of_place_scratch_len(&self) -> usize {
                self.common_data.outofplace_scratch_len
            }
        }
        impl<T: FFTnum> Length for $struct_name<T> {
            #[inline(always)]
            fn len(&self) -> usize {
                self.common_data.len
            }
        }
        impl<T: FFTnum> IsInverse for $struct_name<T> {
            #[inline(always)]
            fn is_inverse(&self) -> bool {
                self.common_data.inverse
            }
        }
    )
}

mod avx_vector;

mod avx32_utils;
mod avx32_butterflies;

mod avx64_utils;
mod avx64_butterflies;

mod avx_mixed_radix;
mod avx_bluesteins;

pub mod avx_planner;

pub use self::avx32_butterflies::{
    Butterfly5Avx,
    Butterfly7Avx,
    MixedRadixAvx4x2,
    MixedRadixAvx3x3,
    MixedRadixAvx4x3,
    MixedRadixAvx4x4,
    MixedRadixAvx4x6,
    MixedRadixAvx3x9,
    MixedRadixAvx4x8,
    MixedRadixAvx4x9,
    MixedRadixAvx4x12,
    MixedRadixAvx6x9,
    MixedRadixAvx8x8,
    MixedRadixAvx6x12,
};

pub use self::avx64_butterflies::{
    Butterfly5Avx64,
    Butterfly7Avx64,
    MixedRadix64Avx4x2,
    MixedRadix64Avx3x3,
    MixedRadix64Avx4x3,
    MixedRadix64Avx4x4,
    MixedRadix64Avx3x6,
    MixedRadix64Avx4x6,
    MixedRadix64Avx3x9,
    MixedRadix64Avx4x8,
    MixedRadix64Avx6x6
};

pub use self::avx_bluesteins::BluesteinsAvx;
pub use self::avx_mixed_radix::{MixedRadix2xnAvx, MixedRadix3xnAvx, MixedRadix4xnAvx, MixedRadix6xnAvx, MixedRadix8xnAvx, MixedRadix9xnAvx, MixedRadix12xnAvx, MixedRadix16xnAvx};