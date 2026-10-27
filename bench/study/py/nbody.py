"""Five-body simulation, five million steps, matching the JS operation order."""
from math import sqrt

PI = 3.141592653589793
SOLAR_MASS = 4 * PI * PI
DAYS = 365.24
STEPS = 7_000_000
x = [0, 4.84143144246472090e00, 8.34336671824457987e00, 1.28943695621391310e01, 1.53796971148509165e01]
y = [0, -1.16032004402742839e00, 4.12479856412430479e00, -1.51111514016986312e01, -2.59193146099879641e01]
z = [0, -1.03622044471123109e-01, -4.03523417114321381e-01, -2.23307578892655734e-01, 1.79258772950371181e-01]
vx = [v * DAYS for v in [0, 1.66007664274403694e-03, -2.76742510726862411e-03, 2.96460137564761618e-03, 2.68067772490389322e-03]]
vy = [v * DAYS for v in [0, 7.69901118419740425e-03, 4.99852801234917238e-03, 2.37847173959480950e-03, 1.62824170038242295e-03]]
vz = [v * DAYS for v in [0, -6.90460016972063023e-05, 2.30417297573763929e-05, -2.96589568540237556e-05, -9.51592254519715870e-05]]
mass = [v * SOLAR_MASS for v in [1, 9.54791938424326609e-04, 2.85885980666130812e-04, 4.36624404335156298e-05, 5.15138902046611451e-05]]


def energy():
    e = 0.0
    for i in range(5):
        e += 0.5 * mass[i] * (vx[i] * vx[i] + vy[i] * vy[i] + vz[i] * vz[i])
        for j in range(i + 1, 5):
            dx, dy, dz = x[i] - x[j], y[i] - y[j], z[i] - z[j]
            e -= mass[i] * mass[j] / sqrt(dx * dx + dy * dy + dz * dz)
    return e


def advance(dt):
    for i in range(5):
        for j in range(i + 1, 5):
            dx, dy, dz = x[i] - x[j], y[i] - y[j], z[i] - z[j]
            d2 = dx * dx + dy * dy + dz * dz
            mag = dt / (d2 * sqrt(d2))
            vx[i] -= dx * mass[j] * mag
            vy[i] -= dy * mass[j] * mag
            vz[i] -= dz * mass[j] * mag
            vx[j] += dx * mass[i] * mag
            vy[j] += dy * mass[i] * mag
            vz[j] += dz * mass[i] * mag
    for i in range(5):
        x[i] += dt * vx[i]
        y[i] += dt * vy[i]
        z[i] += dt * vz[i]


def main():
    px = py = pz = 0.0
    for i in range(5):
        px += vx[i] * mass[i]
        py += vy[i] * mass[i]
        pz += vz[i] * mass[i]
    vx[0], vy[0], vz[0] = -px / SOLAR_MASS, -py / SOLAR_MASS, -pz / SOLAR_MASS
    before = int(energy() * 1e9)
    for _ in range(STEPS):
        advance(0.01)
    print(before, int(energy() * 1e9))


if __name__ == "__main__":
    main()
