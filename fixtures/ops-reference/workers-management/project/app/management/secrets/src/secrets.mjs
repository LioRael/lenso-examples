export function create(binding, scope, configuration) {
  if (typeof binding !== 'string' || binding.length > 32768 || !scope?.run ||
      !configuration || Object.keys(configuration).sort().join(',') !== 'caller_instance,pepper_reference,signing_reference' ||
      configuration.caller_instance !== 'lenso.auth.api-token/default') {
    throw new Error('invalid operators secrets facility');
  }
  const refs = [configuration.signing_reference, configuration.pepper_reference];
  if (refs.some(reference => typeof reference !== 'string' || !reference || reference.length > 256) || refs[0] === refs[1]) {
    throw new Error('invalid operators secret references');
  }
  let values;
  try { values = JSON.parse(binding); } catch { throw new Error('invalid private secret map'); }
  if (!values || Array.isArray(values) || Object.keys(values).sort().join('\0') !== refs.toSorted().join('\0') ||
      refs.some(reference => typeof values[reference] !== 'string' || values[reference].length < 32 || values[reference].length > 16384)) {
    throw new Error('invalid private secret map');
  }
  return Object.freeze({
    resolve(reference, caller) {
      return scope.run(async () => caller === configuration.caller_instance && refs.includes(reference) ? values[reference] : null);
    },
  });
}
