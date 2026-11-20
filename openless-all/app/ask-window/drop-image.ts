const MAX_SOURCE_BYTES = 48 * 1024 * 1024;
const COMPACT_TARGET = 3 * 1024 * 1024;
const MAX_SOURCE_PIXELS = 40_000_000;
const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const JPEG_SIGNATURE = [0xff, 0xd8, 0xff];
const TOO_LARGE = 'This image is too large to prepare safely. Try exporting a smaller copy.';

function dimensions(bytes: Uint8Array, png: boolean): [number, number] | undefined {
  const header = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (png) {
    if (bytes.length >= 24 && header.getUint32(12) === 0x49484452) {
      return [header.getUint32(16), header.getUint32(20)];
    }
    return;
  }
  for (let offset = 2; offset + 4 < bytes.length;) {
    if (bytes[offset++] !== 0xff) return;
    while (bytes[offset] === 0xff) offset++;
    const marker = bytes[offset++];
    if (marker === 0xda || marker === 0xd9 || offset + 2 > bytes.length) return;
    const length = header.getUint16(offset);
    if (length < 2 || offset + length > bytes.length) return;
    if (marker >= 0xc0 && marker <= 0xcf && ![0xc4, 0xc8, 0xcc].includes(marker)) {
      if (length >= 7) return [header.getUint16(offset + 5), header.getUint16(offset + 3)];
      return;
    }
    offset += length;
  }
  return;
}

export async function readDroppedImage(file: File) {
  if (file.size > MAX_SOURCE_BYTES) throw new Error(TOO_LARGE);
  let bytes = new Uint8Array(await file.arrayBuffer());
  const png = PNG_SIGNATURE.every((byte, index) => bytes[index] === byte);
  const jpeg = JPEG_SIGNATURE.every((byte, index) => bytes[index] === byte);
  const size = (png || jpeg) && dimensions(bytes, png);
  if (!size || !size[0] || !size[1]) throw new Error('Drop a valid PNG or JPEG image.');
  const [width, height] = size;
  if (width * height > MAX_SOURCE_PIXELS || Math.max(width, height) > 24000)
    throw new Error(TOO_LARGE);
  let displayName = file.name;
  if (
    bytes.length > COMPACT_TARGET ||
    width * height > 12_000_000 ||
    Math.max(width, height) > 8000
  ) {
    const bitmap = await createImageBitmap(
      new Blob([bytes], { type: png ? 'image/png' : 'image/jpeg' }),
    );
    try {
      const scale = Math.min(1, 2048 / Math.max(bitmap.width, bitmap.height));
      const canvas = document.createElement('canvas');
      canvas.width = Math.max(1, Math.round(bitmap.width * scale));
      canvas.height = Math.max(1, Math.round(bitmap.height * scale));
      const context = canvas.getContext('2d');
      if (!context) throw new Error('Could not compress this image.');
      const encode = (type: string) =>
        new Promise<Blob>((resolve, reject) => {
          canvas.toBlob(
            (blob) => (blob ? resolve(blob) : reject(new Error('Could not compress this image.'))),
            type,
            0.86,
          );
        });
      context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
      let compressed = png ? await encode('image/png') : await encode('image/jpeg');
      if (png && compressed.size > COMPACT_TARGET) {
        context.globalCompositeOperation = 'destination-over';
        context.fillStyle = '#fff';
        context.fillRect(0, 0, canvas.width, canvas.height);
        compressed = await encode('image/jpeg');
      }
      if (compressed.size > 8 * 1024 * 1024) throw new Error(TOO_LARGE);
      if (jpeg || compressed.type === 'image/jpeg')
        displayName = file.name.replace(/\.[^.]+$/, '') + '.jpg';
      bytes = new Uint8Array(await compressed.arrayBuffer());
    } finally {
      bitmap.close();
    }
  }
  const chunks: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 0x4000) {
    chunks.push(String.fromCharCode(...bytes.subarray(offset, offset + 0x4000)));
  }
  return { displayName, dataBase64: btoa(chunks.join('')) };
}
