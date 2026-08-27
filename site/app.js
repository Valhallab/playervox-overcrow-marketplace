"use strict";

const state = { locale: "en", targets: [] };
const catalog = document.getElementById("catalog");
const language = document.getElementById("language");

function decodePayload(value) {
  const padded = value.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(value.length / 4) * 4, "=");
  const bytes = Uint8Array.from(atob(padded), (character) => character.charCodeAt(0));
  return JSON.parse(new TextDecoder().decode(bytes));
}

function localized(target) {
  const available = target.listing.localizations;
  return available.find((text) => text.locale === state.locale)
    || available.find((text) => text.locale === target.manifest.defaultLocale)
    || available[0];
}

function dependencyText(target) {
  if (target.manifest.dependencies.length === 0) {
    return "Standalone package";
  }
  const names = target.manifest.dependencies.map((dependency) => {
    const provider = state.targets.find((candidate) => candidate.manifest.id === dependency.id);
    return provider ? localized(provider).name : "Warframe Worldstate Provider";
  });
  return `Includes dependency: ${names.join(", ")}`;
}

function card(target) {
  const text = localized(target);
  const element = document.createElement("article");
  element.className = "card";
  const name = document.createElement("h2");
  name.textContent = text.name;
  const description = document.createElement("p");
  description.textContent = text.description;
  const languages = document.createElement("p");
  languages.textContent = `Languages: ${target.manifest.availableLocales.join(", ")}`;
  const dependency = document.createElement("p");
  dependency.textContent = dependencyText(target);
  element.append(name, description, languages, dependency);
  return element;
}

function render() {
  catalog.replaceChildren();
  const visible = state.targets.filter((target) => target.manifest.kind !== "provider");
  for (const target of visible) {
    catalog.append(card(target));
  }
}

language.addEventListener("change", () => {
  const locale = language.value === "fr" ? "fr" : "en";
  state.locale = locale;
  document.documentElement.lang = state.locale;
  render();
});

fetch("/marketplace/v1/catalog.json")
  .then((response) => {
    if (!response.ok) throw new Error("catalog unavailable");
    return response.json();
  })
  .then((envelope) => {
    state.targets = decodePayload(envelope.payload).targets;
    render();
  })
  .catch(() => {
    const message = document.createElement("p");
    message.textContent = "Catalog unavailable.";
    catalog.replaceChildren(message);
  });
