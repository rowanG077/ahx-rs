use crate::error::{Error, Result};

pub(crate) trait Input {
    fn read_exact(&mut self, output: &mut [u8]) -> Result<()>;
}

pub(crate) struct SliceInput<'a>(pub &'a [u8]);

impl Input for SliceInput<'_> {
    fn read_exact(&mut self, output: &mut [u8]) -> Result<()> {
        let (input, remaining) = self
            .0
            .split_at_checked(output.len())
            .ok_or(Error::Truncated)?;
        output.copy_from_slice(input);
        self.0 = remaining;
        Ok(())
    }
}

#[cfg(feature = "std")]
pub(crate) struct ReaderInput<R>(pub R);

#[cfg(feature = "std")]
impl<R: std::io::Read> Input for ReaderInput<R> {
    fn read_exact(&mut self, output: &mut [u8]) -> Result<()> {
        self.0.read_exact(output).map_err(Into::into)
    }
}
