"""Prototype interpreter for L2ClientDat structure XML (dist/data/structure).

Used to find which layout version matches this client's decrypted .dat files.
Usage: python datparse.py <structure_dir> <decrypted.bin> <dat name> [pattern]
"""
import struct
import sys
import xml.etree.ElementTree as ET


class Reader:
    def __init__(self, data):
        self.d = data
        self.p = 0

    def take(self, n):
        if self.p + n > len(self.d):
            raise EOFError(self.p)
        b = self.d[self.p:self.p + n]
        self.p += n
        return b

    def compact(self):
        b0 = self.take(1)[0]
        neg = b0 & 0x80
        v = b0 & 0x3F
        if b0 & 0x40:
            shift = 6
            while True:
                b = self.take(1)[0]
                v |= (b & 0x7F) << shift
                shift += 7
                if not b & 0x80 or shift > 27:
                    break
        return -v if neg else v


SIMPLE = {
    'UINT': '<I', 'INT': '<i', 'FLOAT': '<f', 'SHORT': '<h', 'USHORT': '<H',
    'UCHAR': '<B', 'UBYTE': '<B', 'LONG': '<q', 'DOUBLE': '<d', 'MAP_INT': '<I',
    'HEX': '<I', 'RGBA': '<I', 'RGB': '<I',
}


def read_value(r, reader):
    if reader in SIMPLE:
        fmt = SIMPLE[reader]
        return struct.unpack(fmt, r.take(struct.calcsize(fmt)))[0]
    if reader == 'CNTR':
        return r.compact()
    if reader == 'ASCF':
        n = r.compact()
        if n >= 0:
            return r.take(n).rstrip(b'\0').decode('latin-1')
        return r.take(-n * 2).decode('utf-16le').rstrip('\0')
    if reader == 'UNICODE':
        n = struct.unpack('<I', r.take(4))[0]
        return r.take(n).decode('utf-16le', 'replace')
    if reader in ('LOC', 'ROT'):
        return struct.unpack('<3f' if reader == 'LOC' else '<3i', r.take(12))
    raise NotImplementedError(reader)


def run(elems, r, ctx, out):
    for e in elems:
        if e.tag == 'node':
            v = read_value(r, e.get('reader'))
            ctx[e.get('name')] = v
            out.append((e.get('name'), v))
        elif e.tag == 'for':
            size = e.get('size')
            n = ctx[size[1:]] if size.startswith('#') else int(size)
            items = []
            for _ in range(n):
                sub = []
                run(list(e), r, dict(ctx), sub)
                items.append(sub)
            out.append((e.get('name'), items))
        elif e.tag == 'wrapper':
            run(list(e), r, ctx, out)
        elif e.tag == 'if':
            param, val = e.get('param'), e.get('val')
            if str(ctx.get(param)) == val:
                run(list(e), r, ctx, out)
        else:
            pass


def parse(struct_dir, data, dat, pattern=None):
    root = ET.parse(f'{struct_dir}/dats/{dat}.xml').getroot()
    results = []
    for f in root.findall('file'):
        if pattern and f.get('pattern') != pattern:
            continue
        r = Reader(data)
        out = []
        try:
            run(list(f), r, {}, out)
            results.append((f.get('pattern'), r.p, len(data), out))
        except Exception as ex:  # noqa: BLE001 - probing layouts
            results.append((f.get('pattern'), r.p, len(data), repr(ex)))
    return results


if __name__ == '__main__':
    sd, path, dat = sys.argv[1:4]
    pat = sys.argv[4] if len(sys.argv) > 4 else None
    data = open(path, 'rb').read()
    for p, pos, total, out in parse(sd, data, dat, pat):
        status = 'EXACT' if pos == total else ('ok' if not isinstance(out, str) else out[:60])
        print(f'{p:30} consumed {pos}/{total} {status}')
