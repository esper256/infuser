# Thanksgiving artwork

`turkey.png` is the final approved transparent, 256px generated mascot.
The controller faces the turkey; only its back and triggers face the viewer.
Intermediate versions are not shipped.

The theme activates on U.S. Thanksgiving (fourth Thursday of November) using the
device's local calendar. It is a presentation override, never a saved theme ID.
A single midnight timeout and focus/visibility checks restore the saved theme,
color scheme and custom colors afterward. The date is not an API setting or
uploadable asset. Normal appearance catalogs omit this theme outside the holiday.

The existing confetti callback uses a short, non-interactive turkey flock during
the holiday. It honors reduced motion and keeps no persistent effect state.
