### Added

- Place imported glTF lights with a shared EV100 exposure for directional lux and local candela. A local light without an authored range reaches until its exposed light has faded. The glTF preview helper uses these lights, a neutral studio environment and a hue-preserving highlight curve.
- Light a scene with a linear HDR or EXR panorama or cubemap. The renderer prefilters diffuse irradiance and rough specular reflections using the same atlas as the sky and local probes.
- Select the hue-preserving tone curve in Rust, C and C++ to roll off bright colors without clipping their channels independently.
