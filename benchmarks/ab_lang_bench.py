#!/usr/bin/env python3
"""Language A/B: the Python half of benchmarks/ab_lang_bench.ind.

Mirror of the Indent benchmark — keep the two in sync. Each line printed is
"<name> <microseconds per op>"; ab_lang.sh runs both and prints the ratio.
"""
import time

N = 200_000


def noop():
    return 0


def one(a):
    return a


def two(a, b):
    return a + b


def us(t0, t1):
    return (t1 - t0) * 1_000_000.0 / N


# ---------- calls ----------
t0 = time.perf_counter()
for _ in range(N):
    noop()
t1 = time.perf_counter()
print("call_0arg", us(t0, t1))

t2 = time.perf_counter()
acc = 0
for _ in range(N):
    acc = one(1)
t3 = time.perf_counter()
print("call_1arg", us(t2, t3))

t4 = time.perf_counter()
acc2 = 0
for _ in range(N):
    acc2 = two(1, 2)
t5 = time.perf_counter()
print("call_2arg", us(t4, t5))

# ---------- arithmetic + control ----------
t6 = time.perf_counter()
a = 0
for _ in range(N):
    a = a + 1
t7 = time.perf_counter()
print("add_assign", us(t6, t7))

t8 = time.perf_counter()
f = 1.0
for _ in range(N):
    f = f * 1.0000001
t9 = time.perf_counter()
print("float_mul", us(t8, t9))

t10 = time.perf_counter()
c = 0
for _ in range(N):
    if a > 0:
        c = c + 1
t11 = time.perf_counter()
print("if_compare", us(t10, t11))

# ---------- strings ----------
nm = "a"
t12 = time.perf_counter()
for _ in range(N):
    s1 = "hi " + nm
t13 = time.perf_counter()
print("str_concat", us(t12, t13))

t14 = time.perf_counter()
i = 0
for _ in range(N):
    s2 = str(i)
    i = i + 1
t15 = time.perf_counter()
print("int_to_str", us(t14, t15))

t16 = time.perf_counter()
tot2 = 0
for _ in range(N):
    n = int("12345")
    tot2 = tot2 + n
t17 = time.perf_counter()
print("str_to_int", us(t16, t17))

# ---------- dicts ----------
d2 = {}
for i2 in range(100):
    d2["k" + str(i2)] = i2

t18 = time.perf_counter()
s3 = 0
for _ in range(N):
    s3 = s3 + d2["k50"]
t19 = time.perf_counter()
print("dict_read_hit", us(t18, t19))

t20 = time.perf_counter()
d3 = {}
for i3 in range(N):
    d3[str(i3)] = i3
t21 = time.perf_counter()
print("dict_insert", us(t20, t21))

t22 = time.perf_counter()
for i4 in range(N):
    d2[str(i4)] = i4
t23 = time.perf_counter()
print("dict_insert_small", us(t22, t23))

# ---------- lists ----------
t24 = time.perf_counter()
l = []
for i5 in range(N):
    l.append(i5)
t25 = time.perf_counter()
print("list_append", us(t24, t25))

t26 = time.perf_counter()
tot3 = 0
for i6 in range(N):
    tot3 = tot3 + l[i6]
t27 = time.perf_counter()
print("list_index_read", us(t26, t27))

# ---------- allocation ----------
t28 = time.perf_counter()
for i7 in range(N):
    tmp = [i7, i7 + 1, i7 + 2]
t29 = time.perf_counter()
print("list_alloc_small", us(t28, t29))

t30 = time.perf_counter()
for i8 in range(N):
    tmpd = {"a": i8, "b": i8 + 1}
t31 = time.perf_counter()
print("dict_alloc_small", us(t30, t31))

print("checks", a, c, s3, tot3)
