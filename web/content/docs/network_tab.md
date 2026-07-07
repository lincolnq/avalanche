---
title: "Network tab"
date: 2026-07-05T16:30:29-04:00
description: "The simplest way to integrate"
---

The simplest way to make your project integrate with Avalanche is to place a link inside the app, which will appear under the Network tab. All users signed into your server will see the same list of links. The links carry [authentication information](/docs/authentication/) to let the project know who is viewing the page.

The list of projects that appear here is (currently) governed by the `/etc/avalanche/projects.env` file, which contains links to every project that needs to appear in the app.

This structure is quite flexible. Here are some examples of what you can do with it:

* Browseable list of channels/groups you might want to join, with links that add you to those groups[^grouplinks]
* Browseable list of attendees (e.g. for a conference), who you might want to contact, with contact links for those individuals
* Individual project-specific profile page (e.g. let you set various settings having to do with conference, actions, your team, your location, etc)
* Schedules or calendars, public and/or user-specific

On these pages (or any pages) you can place links to conversations. If you want a link that opens a DM with a specific person or bot, you can construct a link that includes a DID:

```
https://go.theavalanche.net/conversation/<did>
```

### Limitations

There are a few limitations as of July 5 2026:

* Links can open DMs only.[^grouplinks]
* If the user has multiple accounts on their device, it doesn't work very well:  conversations will always be opened against the *first* server the user is logged into.

[^grouplinks]: As of this writing you can't construct a link that opens a specific group. This is an obvious gap and will be remedied soon! The current best way to add a person to a specific group is to have a bot in that channel invite the person.

