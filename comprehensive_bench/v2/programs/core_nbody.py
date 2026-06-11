# core_nbody: classic 5-body solar-system simulation (shootout style).
# Prints energy scaled to int (truncated) so output is integer-only.

import math

PI = 3.141592653589793
SOLAR_MASS = 4.0 * PI * PI
DAYS_PER_YEAR = 365.24


class Body:
    def __init__(self, x, y, z, vx, vy, vz, mass):
        self.x = x
        self.y = y
        self.z = z
        self.vx = vx
        self.vy = vy
        self.vz = vz
        self.mass = mass


def make_bodies():
    return [
        # Sun
        Body(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SOLAR_MASS),
        # Jupiter
        Body(
            4.84143144246472090,
            -1.16032004402742839,
            -0.103622044471123109,
            0.00166007664274403694 * DAYS_PER_YEAR,
            0.00769901118419740425 * DAYS_PER_YEAR,
            -0.0000690460016972063023 * DAYS_PER_YEAR,
            0.000954791938424326609 * SOLAR_MASS),
        # Saturn
        Body(
            8.34336671824457987,
            4.12479856412430479,
            -0.403523417114321381,
            -0.00276742510726862411 * DAYS_PER_YEAR,
            0.00499852801234917238 * DAYS_PER_YEAR,
            0.0000230417297573763929 * DAYS_PER_YEAR,
            0.000285885980666130812 * SOLAR_MASS),
        # Uranus
        Body(
            12.8943695621391310,
            -15.1111514016986312,
            -0.223307578892655734,
            0.00296460137564761618 * DAYS_PER_YEAR,
            0.00237847173959480950 * DAYS_PER_YEAR,
            -0.0000296589568540237556 * DAYS_PER_YEAR,
            0.0000436624404335156298 * SOLAR_MASS),
        # Neptune
        Body(
            15.3796971148509165,
            -25.9193146099879641,
            0.179258772950371181,
            0.00268067772490389322 * DAYS_PER_YEAR,
            0.00162824170038242295 * DAYS_PER_YEAR,
            -0.0000951592254519715870 * DAYS_PER_YEAR,
            0.0000515138902046611451 * SOLAR_MASS),
    ]


def offset_momentum(bodies):
    px = py = pz = 0.0
    for b in bodies:
        px += b.vx * b.mass
        py += b.vy * b.mass
        pz += b.vz * b.mass
    sun = bodies[0]
    sun.vx = 0.0 - px / SOLAR_MASS
    sun.vy = 0.0 - py / SOLAR_MASS
    sun.vz = 0.0 - pz / SOLAR_MASS


def advance(bodies, dt):
    n = len(bodies)
    for i in range(n):
        bi = bodies[i]
        for j in range(i + 1, n):
            bj = bodies[j]
            dx = bi.x - bj.x
            dy = bi.y - bj.y
            dz = bi.z - bj.z
            d2 = dx * dx + dy * dy + dz * dz
            mag = dt / (d2 * math.sqrt(d2))
            bjm = bj.mass * mag
            bim = bi.mass * mag
            bi.vx = bi.vx - dx * bjm
            bi.vy = bi.vy - dy * bjm
            bi.vz = bi.vz - dz * bjm
            bj.vx = bj.vx + dx * bim
            bj.vy = bj.vy + dy * bim
            bj.vz = bj.vz + dz * bim
    for b in bodies:
        b.x = b.x + dt * b.vx
        b.y = b.y + dt * b.vy
        b.z = b.z + dt * b.vz


def energy(bodies):
    e = 0.0
    n = len(bodies)
    for i in range(n):
        bi = bodies[i]
        e = e + 0.5 * bi.mass * (bi.vx * bi.vx + bi.vy * bi.vy + bi.vz * bi.vz)
        for j in range(i + 1, n):
            bj = bodies[j]
            dx = bi.x - bj.x
            dy = bi.y - bj.y
            dz = bi.z - bj.z
            e = e - (bi.mass * bj.mass) / math.sqrt(dx * dx + dy * dy + dz * dz)
    return e


def main():
    bodies = make_bodies()
    offset_momentum(bodies)
    e0 = energy(bodies)
    for _ in range(20000):
        advance(bodies, 0.01)
    e1 = energy(bodies)
    print(f"nbody_energy_start={int(e0 * 1000000000.0)}")
    print(f"nbody_energy_end={int(e1 * 1000000000.0)}")


if __name__ == "__main__":
    main()
