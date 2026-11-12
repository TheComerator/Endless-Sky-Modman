/**
 * Lines from Endless Sky's own story text, shown one at a time on the start-up card.
 *
 * Source: the game's data files (GPL-3+, © Michael Zahniser and contributors), credited on the
 * card. Each was chosen to read well on its own and to give nothing away about the story:
 * generic spaceport chatter, jokes and early-game scenes only. Wording is exactly the game's
 * (a leading in-game emotion tag such as "Irony." is dropped). Before adding one, check that it
 * isn't from a late-game or plot-twist scene.
 */
export interface Quote {
  text: string;
  /** Who says it: a character's name and role, or the in-game label ("a longcow rancher"). */
  credit: string;
}

export const QUOTES: readonly Quote[] = [
  {
    text: "Risky, why? I don't see what can go wrong. The system is quite well designed. I'm so proud of my team.",
    credit: "Veronica Oxygenpocket, shipyard designer",
  },
  {
    text: "Don't you worry! Why, that Ice-Crawler is the safest place in all of Nifel! Those other accidents were flukes, and what happened to her sister Ice-Crawler, Gudrun, was entirely unforeseeable.",
    credit: "a tour saleswoman on Nifel",
  },
  {
    text: "You may die, or maybe not. Signals vork strange here at times. Good luck and remember. No refund.",
    credit: "Hjlod, Skadenga tour guide",
  },
  {
    text: "Stopped using vaccines on longcows, long ago we did. Remember which leg to apply them on, we never could.",
    credit: "a longcow rancher",
  },
  {
    text: "You die almost instantly, but you do have a few microseconds to reflect on the wisdom of mocking a man that is trying to fire a gun at you.",
    credit: "narration",
  },
  {
    text: "We learned years ago that we need to end every concert with a sad song. Otherwise the fans leave with way too much energy and start destroying public property.",
    credit: "Becca, band stage manager",
  },
  {
    text: "It's not actually haunted, of course. Those are just stories. Do you think I'd send an entire film crew and all their expensive equipment to a place that was actually dangerous?",
    credit: "Likeet, Hai film director",
  },
  {
    text: "Someone with more money than sense asked me to refill the hyperspace relays, because their signal was low.",
    credit: "a hyperspace relay engineer",
  },
  {
    text: "For the possible cost of a few lives and some bad PR, we can boost returns to around 8200 credits a day, or about 30% a year.",
    credit: "Howard, Syndicate investment salesman",
  },
  {
    text: "He who can, creates; he who cannot, transports. Sorry, no offense meant to you.",
    credit: "a painter",
  },
  {
    text: "The only reason every freighter isn't automated these days is piracy. Thank you, pirates!",
    credit: "an Avgi merchant",
  },
  {
    text: "Nonsense, this formula is perfectly safe! A serum for the health, proven to kill over three thousand forms of cancer, you should know.",
    credit: "an Avgi snake-oil peddler",
  },
  {
    text: 'Someone met a djinn and said "leave me alone." That person is now stranded on an uninhabited planet.',
    credit: "a passing ship's hail",
  },
  {
    text: "It is too late. Ice-Slug has you. Vhat you do now is die. Horribly.",
    credit: "Hjlod, Skadenga tour guide",
  },
  {
    text: "Difficult sometimes it is, to be a mediator. If careful with my words I am not, a Saryd might think an Arach talking about longcows has insulted their mother.",
    credit: "a Heliarch agent",
  },
  {
    text: "Travel to new places, I used to, but infinite, our planets are not, so seen them all by now I have.",
    credit: "a local Saryd",
  },
  {
    text: "Trying to be inconspicuous is admirable. Being oblivious to the fact that you stand out like a star in empty space is not.",
    credit: "Taely, Remnant prefect",
  },
  {
    text: "It only costs 40 credits, but because you have done a horrible job at managing your finances you do not have even that much cash on hand right now.",
    credit: "narration",
  },
  {
    text: "You could set off a nuke on the far side of that world and the miners would probably sleep right through it.",
    credit: "Mr. Eyes",
  },
  {
    text: "You always see bright up-and-coming students claim to have invented a new kind of faster than light drive. Eighty years ago, one of them was actually right!",
    credit: "an Avgi physicist",
  },
  {
    text: "The galaxy is three-dimensional, but you wouldn't know it from the way everyone draws their maps!",
    credit: "an Avgi astronomer",
  },
  {
    text: "He's got the Bloom. He'll be dead in a few weeks.",
    credit: "an old man in a Bloodsea spaceport",
  },
  {
    text: "Hyperspace links don't just allow faster-than-light travel. They also allow near-instantaneous communication, if you have the right sort of relay on both ends.",
    credit: "a hyperspace relay engineer",
  },
  {
    text: "Mirja-Mirja, the most loyal of friends and gentlest of animals. Such stuff as dreams are made on.",
    credit: "a pet memorial message",
  },
  {
    text: "Lucky you are, that under our jurisdiction this happened. Were it the Quarg's, stranded you would remain.",
    credit: "a Saryd",
  },
  {
    text: "Yes, the team sent me some videos of the battle. First-rate carnage. I was very pleased.",
    credit: '"Barmy Edward", weapons engineer',
  },
  {
    text: "To know is to lead, to study is to know, to serve is to study, to lead is to serve.",
    credit: "a Hai academy inscription",
  },
  {
    text: "Honesty is good. It is hard to learn when we are not willing to admit that we do not know something.",
    credit: "Dawn, Remnant researcher",
  },
  {
    text: "Apparently the goat ate the cabbage, and shortly after that the wolf ate the goat.",
    credit: "narration",
  },
  {
    text: "The galaxy is a big and dangerous place, and not for the fainthearted. It'll be quite a while until you can make the universe your oyster.",
    credit: "an old spaceport veteran",
  },
];

/** The index shown last, so reopening the card straight away never repeats it. */
let lastIndex = -1;

/**
 * A random quote, never the same one twice in a row. `random` is injectable so tests can
 * choose; it must return a number in [0, 1) like `Math.random`.
 */
export function pickQuote(random: () => number = Math.random): Quote {
  let index = Math.min(QUOTES.length - 1, Math.floor(random() * QUOTES.length));
  if (index === lastIndex && QUOTES.length > 1) {
    index = (index + 1) % QUOTES.length;
  }
  lastIndex = index;
  return QUOTES[index];
}

/** Test hook: forgets the last quote shown. */
export function resetLastQuote(): void {
  lastIndex = -1;
}
