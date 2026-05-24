# Python script to generate a 4x4 red square PNG file.
import zlib
import struct
import os

def make_png(width, height, rgb_color):
    # PNG signature
    png = b'\x89PNG\r\n\x1a\n'
    
    # IHDR chunk
    # width (4 bytes), height (4 bytes), bit depth (1 byte, 8), color type (1 byte, 2 = RGB),
    # compression method (1 byte, 0), filter method (1 byte, 0), interlace method (1 byte, 0)
    ihdr_data = struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)
    
    def chunk(tag, data):
        return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data))
        
    png += chunk(b'IHDR', ihdr_data)
    
    # IDAT chunk
    # For each row, 1 filter byte (0) followed by 3 bytes per pixel (R, G, B)
    row = b'\x00' + bytes(rgb_color) * width
    raw_data = row * height
    idat_data = zlib.compress(raw_data)
    png += chunk(b'IDAT', idat_data)
    
    # IEND chunk
    png += chunk(b'IEND', b'')
    return png

if __name__ == '__main__':
    # Write a 4x4 red (255, 0, 0) PNG
    data = make_png(4, 4, (255, 0, 0))
    target_path = os.path.join(os.path.dirname(__file__), 'red_square.png')
    with open(target_path, 'wb') as f:
        f.write(data)
    print(f"Generated {target_path}")
