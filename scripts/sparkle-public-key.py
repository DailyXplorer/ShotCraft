import base64, hashlib, os, sys

"""Prints the public key (base64) that goes with the Sparkle private key in $SPARKLE_ED_KEY.

Sparkle's tools only sign with the private key, so this is how package-release.sh checks that the
secret matches SUPublicEDKey. Ed25519 as in RFC 8032, section 6, with the standard library only.
"""
p = 2**255 - 19
d = -121665 * pow(121666, p - 2, p) % p
gy = 4 * pow(5, p - 2, p) % p


def recover_x(y, sign):
    x = pow((y * y - 1) * pow(d * y * y + 1, p - 2, p) % p, (p + 3) // 8, p)
    if (x * x - (y * y - 1) * pow(d * y * y + 1, p - 2, p)) % p:
        x = x * pow(2, (p - 1) // 4, p) % p
    return p - x if x & 1 != sign else x


G = (recover_x(gy, 0), gy, 1, recover_x(gy, 0) * gy % p)


def add(a, b):
    A, B = (a[1] - a[0]) * (b[1] - b[0]) % p, (a[1] + a[0]) * (b[1] + b[0]) % p
    C, D = 2 * a[3] * b[3] * d % p, 2 * a[2] * b[2] % p
    E, F, H, I = B - A, D - C, B + A, D + C
    return (E * F % p, H * I % p, F * I % p, E * H % p)


def public_key(scalar):
    point, base = (0, 1, 1, 0), G
    while scalar:
        if scalar & 1:
            point = add(point, base)
        base = add(base, base)
        scalar >>= 1
    zinv = pow(point[2], p - 2, p)
    x, y = point[0] * zinv % p, point[1] * zinv % p
    return (y | ((x & 1) << 255)).to_bytes(32, "little")


key = base64.b64decode(os.environ["SPARKLE_ED_KEY"].strip())
if len(key) == 32:  # a seed
    scalar = bytearray(hashlib.sha512(key).digest()[:32])
elif len(key) in (64, 96):  # an expanded key, with the public key appended in the 96 byte form
    scalar = bytearray(key[:32])
else:
    sys.exit(f"SPARKLE_ED_KEY decodes to {len(key)} bytes, expected 32, 64 or 96")
scalar[0] &= 248
scalar[31] &= 127
scalar[31] |= 64
print(base64.b64encode(public_key(int.from_bytes(scalar, "little"))).decode())
