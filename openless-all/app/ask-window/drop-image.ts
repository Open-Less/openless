const MAX_IMAGE_BYTES = 8 * 1024 * 1024;
const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const JPEG_SIGNATURE = [0xff, 0xd8, 0xff];

export async function readDroppedImage(file: File) {
  if (file.size > MAX_IMAGE_BYTES) throw new Error('Images must be 8 MiB or smaller.');
  const signature = new Uint8Array(await file.slice(0, PNG_SIGNATURE.length).arrayBuffer());
  if (
    !PNG_SIGNATURE.every((byte, index) => signature[index] === byte) &&
    !JPEG_SIGNATURE.every((byte, index) => signature[index] === byte)
  ) {
    throw new Error('Drop a PNG or JPEG image.');
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  const chunks: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 0x4000) {
    chunks.push(String.fromCharCode(...bytes.subarray(offset, offset + 0x4000)));
  }
  return { displayName: file.name, dataBase64: btoa(chunks.join('')) };
}
