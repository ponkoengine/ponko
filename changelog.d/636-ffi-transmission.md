<!-- area: C and C++ -->
### Added
- C and C++: glass and diffuse transmission can be authored. `PonkoMaterialDescEx`
  version 4 adds `transmission` (`PonkoMaterialTransmission`, `ponko::material_transmission`):
  the transmission factor, volume thickness and attenuation, dispersion, diffuse
  transmission and approximate scatter, with their six maps. Programs built against
  version 3 keep working and get no transmission.
