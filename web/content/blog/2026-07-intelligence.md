---
title: "Conversation intelligence"
date: 2026-07-16
author: "Lincoln Quirk"
description: "How to take control of your inbox"
---

*Implementation status: Speculative, seeking feedback.*

Signal has no filtering whatsoever. If you're in a Signal group, you can mute the group meaning you don't get notified of messages. But you can't stop the group from appearing at the top of your conversations list, showing as unread, etc.

We can do so much better than this. More than a decade ago (2013) Gmail built the auto-classifying "tabs." I absolutely love this feature and have been using it nonstop since it became available. Why doesn't Signal have a feature like this? Why can't I get the quality of classification that Gmail has had since before deep learning existed?

## End-to-end encryption

There are reasons that seem plausible to me: for example, Signal is end-to-end encrypted, so the server can't read your messages. If Signal were to classify messages it would have to happen on the user's device, and phones aren't that powerful... or are they?

## On-device models

Since 2024, Apple has shipped phones with what they're calling "Apple Intelligence," a stack of models that can do real work on mobile devices, using very little power. And now those models are available for any developer to make use of.

Apple uses this to implement their "Reduce Interruptions" feature, which only buzzes your phone for notifications if it thinks that the notification is important and time-sensitive. I've been using this feature for the last year and it's pretty good, and it is definitely classifying each message using its _content_: a message that has time-sensitive things like "party at 5pm!" is much more likely to buzz my phone than one asking, "how you doin?"

## Beating Apple and Google at their own games

Toss out Gmail's tabs. With Avalanche, on devices that have models like this, you'll be able to _specify your own tabs_ and what criteria should sort your messages.

In a bunch of action groups? Bucket all your action groups in their own tab, and notify only when there's something in the next day.

I'm in dozens of social groups, most of which are quite active, and only occasionally is there anything urgent in any of them. We'll be able to "turn down the notification volume" on social groups, poking stuff to your attention if someone mentions your name, or a cool event you might want to attend.

For work, we can figure out (or you can tell us) which of your groups are work related and only notify you during work hours. Et cetera.

## Device support

Apple ships Apple Intelligence on all new iPhones and Macs (and many iPads) since 2025, and there's also a developer API which can be used to build this.

Claude tells me that recent high-end Samsung and Google Pixel phones have an AI chip that's roughly on par with whatever Apple is shipping, and Google apparently also makes a decent MLKit API that uses a shared on-device Gemini Nano model. This should be available on Pixel 9+, Galaxy S24+, and Xiaomi 15+.[^lowtier] 

[^lowtier]: There are a ton of lower-tier phones shipped today under Pixel, Galaxy and Xiaomi brands that are not powerful enough to run these models---some of which are frustratingly marketed as "AI phones". E.g., the A series on both Pixel and Samsung does *not* support MLKit GenAI. [There's a list on the Google Developers page here.](https://developers.google.com/ml-kit/genai#device-support)

Why only these phones? While it's theoretically possible to make this kind of thing work on lower-end devices, I wouldn't want to ship a feature that uses a ton of space or battery, especially being just me and having very limited testing resources. I'm going to outsource the testing work to Google/Apple instead. If they say a language model works on this phone I'll trust them.

## Architecture

We'll be using Apple/Google's on-device models.

**Why on-device?** For privacy reasons I refuse to send your messages off your devices. I use cloud AI plenty in my work, but I definitely wouldn't send my private messages to AI, and I don't think you should let me (or anyone!) analyze your messages from your encrypted communication platform.

There are a number of architectural ideas and I haven't settled on a specific plan yet---feedback is welcome:

From a UX perspective, each conversation will be bucketed into exactly one category. The categories will be like Gmail tabs at the top of your inbox. Categories will have different notification defaults, and you can (e.g.) automute all conversations classified as social, if you wish.

* **Conversations** should probably have a (fairly) stable classification. There are few enough conversations that it is reasonable for users to classify them manually, but we can speed them up and make their lives easier by helping them out with this. 
    * Inputs to classification include participant count and participant list, titles, message history, and bot vs human activity. 
    * Output is which category the conversation is in.
* **Messages** just live in conversations. 
    * Input is the sender, the message body, and maybe some prior messages in the conversation history.
    * Output is an override for category defaults---e.g., if someone in a social conversation mentions you, a message classifier can say "this message is probably worth flagging to the user" and then we can highlight that message in some way that will make you more likely to see it. Or, one potential output could be "this message is a surprising one to receive in this category and might be worth reclassifying the conversation."

## But I don't have a fancy phone

So if your device isn't powerful enough, what do you get? Well, you will still be able to get tabs and you can still manually classify your conversations. And that might actually be quite good, a sizeable step up over Whatsapp or Signal. It's not nearly as magical as the auto-classification, and will not have per-message overrides, but it will help you keep control over your inbox.

