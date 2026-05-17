def build_a(n):
    return [float(i) for i in range(n)]

def build_b(n):
    return [float(i) * 2.0 for i in range(n)]

def dot(a, b):
    s = 0.0
    for i in range(len(a)):
        s += a[i] * b[i]
    return s

a = build_a(1000000)
b = build_b(1000000)
print(f"dot={dot(a, b)}")
