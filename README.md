# OMEN fan controller in rust
This is my first rust project, and it is largely based on [this](https://github.com/alou-S/omen-fan) project. I wanted something that could easily be run as a service and most importantly lets me prevent the very annoying pulsing sound of the fans of my HP OMEN 17. 

Example configuration file (/etc/omen-fan/config.toml):
```toml
[service]
temp_curve = [[45, 57], [50, 60], [55, 65], [60, 70]]
speed_curve = [0, 25, 50, 100]
poll_interval = 500 # In milliseconds
```
The values in `TEMP_CURVE` serve as ranges where the speed of the fan will only change once it drops below the bottom value. The values in speed curve represent that percent of the max speed the fans will go at the different ranges.

Created and tested for the OMEN by HP Laptop 17-ck1xxx. Use on other laptops at your own risk. Will probably require changing the offset values in the source code.
