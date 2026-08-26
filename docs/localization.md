# Localization

Every package declares one default locale and the exact list of available
locales. Community creators are not required to provide translations beyond
the default locale, but listing pages must always show the available languages.

Locale identifiers and translation files are validated and bounded. The host
uses the selected application locale when the package provides it, otherwise
it falls back to the package default. Missing keys also fall back to the
default locale; creator text is never interpreted as HTML.

Official PlayerVox proof-of-concept widgets provide complete English and
French metadata and UI. English is their default. This requirement for
official examples does not impose French on community packages.
