# AI Interview Assistant 🎙️🤖

An intelligent assistant designed to help you during online technical interviews. It listens to your Google Meet or Zoom calls in real-time, processes the conversation, and provides contextual hints and answers on the fly — based on your own background and experience.

## Features

- **Real-Time Audio Capture** — Captures both microphone and system audio concurrently using WASAPI (Windows).
- **Voice Activity Detection** — Silero VAD detects when the interviewer finishes speaking before triggering a response.
- **Speech-to-Text** — Deepgram transcribes both speakers in real-time via WebSocket.
- **RAG Pipeline** — Your personal context is vectorized at startup and retrieved semantically for each question.
- **AI-Powered Responses** — Groq (llama-3.1-8b-instant) generates concise bullet points you can use to answer.
- **Transcript** — Every turn (interviewer, candidate, and AI) is saved to `transcript.txt`.
- **Pause / Resume** — Press `F9` at any time to pause and resume audio capture.

## Tech Stack

| Component | Technology |
|---|---|
| Audio capture | WASAPI (Windows native) |
| Voice detection | Silero VAD (local ONNX model) |
| Speech-to-text | Deepgram nova-2 |
| Embeddings | Voyage AI (voyage-3-lite, 512 dims) |
| Vector store | In-memory cosine similarity |
| LLM | Groq — llama-3.1-8b-instant |

## Requirements

- Windows (WASAPI dependency)
- Rust toolchain (`cargo`)
- A virtual audio cable or loopback driver to capture system audio (e.g. [VB-Cable](https://vb-audio.com/Cable/))

## Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/juanmas-hub/ai-interview-assistant
   cd ai-interview-assistant
   ```

2. Create a `.env` file in the project root:
   ```env
   DEEPGRAM_API_KEY=your_deepgram_key
   VOYAGE_API_KEY=your_voyage_key
   GROQ_API_KEY=your_groq_key
   ```

3. Get your API keys:
   - **Deepgram** → [console.deepgram.com](https://console.deepgram.com) (free tier available)
   - **Voyage AI** → [dash.voyageai.com](https://dash.voyageai.com) (free tier: 200M tokens/month)
   - **Groq** → [console.groq.com](https://console.groq.com) (free tier available)

4. Build and run:
   ```bash
   cargo run
   ```

## Usage

When the program starts, it will ask you to enter your personal context — one idea per line:

```
Tell us about yourself before starting the interview.
Write one idea per line. Examples:
  I work at Caelum as a Full Stack Engineer using Go and React
  I built a microservices-based ticketing app called Nexus using Go and TypeScript
  I have experience with hexagonal architecture and DDD

Press Enter twice when you're done.
```

The more specific your context, the better the responses. Include:
- Current role and company
- Projects you've worked on and the technologies used
- Your main tech stack
- Architecture patterns you've applied

Once setup is complete, the assistant runs silently in the background. When the interviewer finishes a question, the AI automatically generates a response in the console.

## Controls

| Key | Action |
|---|---|
| `F9` | Pause / resume audio capture |
| `Ctrl+C` | Shut down |

## Output

All conversation turns are saved to `transcript.txt` in the project root:

```
[Interviewer]: Tell me about your experience with microservices.
[AI]: - Built Nexus, a ticket booking platform fully based on microservices using Go and TypeScript...
[User]: I've worked with microservices in my Nexus project...
```