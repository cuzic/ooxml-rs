export const defaultDigitWidth = 6;

export function columnPixels(width, digits) {
  return Math.floor((width + Math.round(128 / digits) / 256) * digits);
}

export function columnCharacters(pixels, digits) {
  return Math.floor((pixels - 5) / digits * 100 + 0.5) / 100;
}

export function columnWidth(characters, digits) {
  return Math.round(((characters * digits + 5) / digits) * 256) / 256;
}

export function inferDigitWidth(width) {
  const error = (digits) => Math.abs(width - columnWidth(columnCharacters(columnPixels(width, digits), digits), digits));
  let best = defaultDigitWidth;
  let distance = error(best);
  if (distance > 0.005) {
    for (let digits = 1; digits < 15; digits += 1) {
      const difference = error(digits);
      if (difference <= distance) {
        best = digits;
        distance = difference;
      }
    }
  }
  return best;
}
