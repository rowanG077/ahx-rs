//! Link the decoder without a standard library or global allocator.
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub fn decode_ahx(input: &[u8], pcm: &mut [i16]) -> Result<u64, ahx_rs::Error> {
    let mut decoder = ahx_rs::SliceDecoder::with_buffer(input, pcm)?;
    let mut samples = 0;
    while let Some(block) = decoder.next_block()? {
        samples += block.len() as u64;
    }
    Ok(samples)
}

#[no_mangle]
pub fn decode_ahx_packet(input: &[u8], pcm: &mut [i16]) -> Result<usize, ahx_rs::Error> {
    Ok(ahx_rs::PacketDecoder::new()
        .decode_into(input, pcm)?
        .consumed_bytes())
}
