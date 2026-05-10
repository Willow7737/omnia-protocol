import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ArrowRight, Check, Zap, Shield, Globe, Brain, Lock, Infinity } from "lucide-react";
import { useState } from "react";

export default function Home() {
  const [activeTab, setActiveTab] = useState("vision");

  return (
    <div className="min-h-screen bg-gradient-to-br from-slate-950 via-purple-900 to-slate-950">
      {/* Navigation */}
      <nav className="fixed top-0 w-full z-50 backdrop-blur-md bg-slate-950/50 border-b border-purple-500/20">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-4 flex justify-between items-center">
          <div className="flex items-center gap-2">
            <div className="w-10 h-10 bg-gradient-to-br from-purple-500 to-pink-500 rounded-lg flex items-center justify-center">
              <Infinity className="w-6 h-6 text-white" />
            </div>
            <span className="text-xl font-bold text-white">Omnia Protocol</span>
          </div>
          <div className="hidden md:flex gap-8">
            <a href="#vision" className="text-gray-300 hover:text-white transition">Vision</a>
            <a href="#architecture" className="text-gray-300 hover:text-white transition">Architecture</a>
            <a href="#features" className="text-gray-300 hover:text-white transition">Features</a>
            <a href="#roadmap" className="text-gray-300 hover:text-white transition">Roadmap</a>
          </div>
          <Button className="bg-gradient-to-r from-purple-600 to-pink-600 hover:from-purple-700 hover:to-pink-700">
            Get Started
          </Button>
        </div>
      </nav>

      {/* Hero Section */}
      <section className="pt-32 pb-20 px-4 sm:px-6 lg:px-8">
        <div className="max-w-7xl mx-auto">
          <div className="text-center mb-16">
            <div className="inline-block mb-6 px-4 py-2 bg-purple-500/10 border border-purple-500/30 rounded-full">
              <span className="text-purple-300 text-sm font-medium">The Future of Coordination</span>
            </div>
            <h1 className="text-5xl sm:text-7xl font-bold text-white mb-6 leading-tight">
              The Universal
              <span className="bg-gradient-to-r from-purple-400 via-pink-400 to-purple-400 bg-clip-text text-transparent"> Coordination Layer</span>
              <br />
              for Reality
            </h1>
            <p className="text-xl text-gray-300 mb-8 max-w-2xl mx-auto leading-relaxed">
              Replace trust with mathematics. Enable value exchange, identity verification, and physical-digital fusion without intermediaries. Omnia is the infrastructure for a future where every human and AI agent can participate as equals.
            </p>
            <div className="flex flex-col sm:flex-row gap-4 justify-center">
              <Button size="lg" className="bg-gradient-to-r from-purple-600 to-pink-600 hover:from-purple-700 hover:to-pink-700 text-white">
                Explore Documentation <ArrowRight className="ml-2 w-4 h-4" />
              </Button>
              <Button size="lg" variant="outline" className="border-purple-500/30 text-white hover:bg-purple-500/10">
                View on GitHub
              </Button>
            </div>
          </div>

          {/* Stats */}
          <div className="grid grid-cols-1 md:grid-cols-4 gap-6 mt-20">
            {[
              { label: "Throughput", value: "10,000+ TPS", icon: Zap },
              { label: "Latency", value: "1-5 seconds", icon: Infinity },
              { label: "Security", value: "Quantum-Ready", icon: Shield },
              { label: "Scale", value: "Earth to Mars", icon: Globe },
            ].map((stat, i) => {
              const Icon = stat.icon;
              return (
                <div key={i} className="bg-gradient-to-br from-purple-500/10 to-pink-500/10 border border-purple-500/20 rounded-lg p-6 text-center">
                  <Icon className="w-8 h-8 text-purple-400 mx-auto mb-3" />
                  <p className="text-gray-400 text-sm mb-2">{stat.label}</p>
                  <p className="text-2xl font-bold text-white">{stat.value}</p>
                </div>
              );
            })}
          </div>
        </div>
      </section>

      {/* Vision Section */}
      <section id="vision" className="py-20 px-4 sm:px-6 lg:px-8 border-t border-purple-500/10">
        <div className="max-w-7xl mx-auto">
          <h2 className="text-4xl font-bold text-white mb-12 text-center">Why Omnia?</h2>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-8">
            {[
              {
                title: "1.7 Billion Unbanked",
                description: "Anyone with a phone can participate—no bank account needed",
                icon: Globe,
              },
              {
                title: "Data Exploitation",
                description: "You control your data; zero-knowledge proofs prove things without revealing them",
                icon: Lock,
              },
              {
                title: "Opaque Supply Chains",
                description: "Every physical item has a cryptographic birth certificate",
                icon: Shield,
              },
              {
                title: "Centralized AI",
                description: "Distributed training lets everyone contribute and share rewards",
                icon: Brain,
              },
            ].map((item, i) => {
              const Icon = item.icon;
              return (
                <Card key={i} className="bg-gradient-to-br from-purple-500/10 to-pink-500/10 border-purple-500/20 hover:border-purple-500/40 transition">
                  <CardHeader>
                    <Icon className="w-8 h-8 text-purple-400 mb-4" />
                    <CardTitle className="text-white">{item.title}</CardTitle>
                  </CardHeader>
                  <CardContent>
                    <p className="text-gray-300">{item.description}</p>
                  </CardContent>
                </Card>
              );
            })}
          </div>
        </div>
      </section>

      {/* Architecture Section */}
      <section id="architecture" className="py-20 px-4 sm:px-6 lg:px-8 bg-gradient-to-br from-slate-900/50 to-purple-900/20 border-t border-purple-500/10">
        <div className="max-w-7xl mx-auto">
          <h2 className="text-4xl font-bold text-white mb-4 text-center">Five-Layer Architecture</h2>
          <p className="text-gray-400 text-center mb-12 max-w-2xl mx-auto">
            Omnia is built on five interconnected layers, each solving a critical problem in decentralized coordination.
          </p>

          <div className="space-y-4">
            {[
              {
                layer: "Layer 5",
                title: "Economic Layer",
                description: "Universal Basic Compute, Retroactive Public Goods Funding, Adaptive Monetary Policy",
                color: "from-purple-600 to-pink-600",
              },
              {
                layer: "Layer 4",
                title: "Identity Layer",
                description: "Decentralized Identifiers, Verifiable Credentials, Reputation System, Social Recovery",
                color: "from-pink-600 to-red-600",
              },
              {
                layer: "Layer 3",
                title: "Binding Layer",
                description: "Physical Anchoring: RF Fingerprinting, Quantum Sealing, Gravitational Timestamps, Biometric Binding, Satellite Mesh",
                color: "from-red-600 to-orange-600",
              },
              {
                layer: "Layer 2",
                title: "Domain Shards",
                description: "Financial, Computational, Physical, Biological, Energy, Temporal, Identity",
                color: "from-orange-600 to-yellow-600",
              },
              {
                layer: "Layer 1",
                title: "The Substrate",
                description: "Physics-Aware Consensus: Causal Graph, Vector Clocks, CRDTs, Relativistic Boundaries",
                color: "from-yellow-600 to-green-600",
              },
            ].map((item, i) => (
              <div key={i} className={`bg-gradient-to-r ${item.color} p-0.5 rounded-lg`}>
                <div className="bg-slate-950 rounded-lg p-6">
                  <div className="flex items-start gap-4">
                    <div className={`bg-gradient-to-r ${item.color} rounded-lg px-4 py-2 flex-shrink-0`}>
                      <span className="text-white font-bold text-sm">{item.layer}</span>
                    </div>
                    <div>
                      <h3 className="text-xl font-bold text-white mb-2">{item.title}</h3>
                      <p className="text-gray-300">{item.description}</p>
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Features Section */}
      <section id="features" className="py-20 px-4 sm:px-6 lg:px-8 border-t border-purple-500/10">
        <div className="max-w-7xl mx-auto">
          <h2 className="text-4xl font-bold text-white mb-12 text-center">Key Features</h2>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
            {[
              {
                title: "Causal Graph Consensus",
                description: "Process independent transactions in parallel, not sequentially. 1000x faster than traditional blockchains.",
              },
              {
                title: "Zero-Knowledge Proofs",
                description: "Prove things about yourself without revealing underlying information. Privacy by design.",
              },
              {
                title: "Physical Anchoring",
                description: "Connect digital transactions to physical reality without trusted intermediaries.",
              },
              {
                title: "Self-Sovereign Identity",
                description: "You own your identity forever. No company or government can revoke it.",
              },
              {
                title: "Universal Basic Compute",
                description: "Everyone gets free monthly quota. Participation doesn't require money.",
              },
              {
                title: "Interplanetary Scale",
                description: "Works on Earth, Mars, and beyond. Designed for 22-minute communication delays.",
              },
            ].map((feature, i) => (
              <Card key={i} className="bg-gradient-to-br from-purple-500/5 to-pink-500/5 border-purple-500/20 hover:border-purple-500/40 transition">
                <CardHeader>
                  <CardTitle className="text-white flex items-center gap-2">
                    <Check className="w-5 h-5 text-green-400" />
                    {feature.title}
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  <p className="text-gray-300">{feature.description}</p>
                </CardContent>
              </Card>
            ))}
          </div>
        </div>
      </section>

      {/* Roadmap Section */}
      <section id="roadmap" className="py-20 px-4 sm:px-6 lg:px-8 bg-gradient-to-br from-slate-900/50 to-purple-900/20 border-t border-purple-500/10">
        <div className="max-w-7xl mx-auto">
          <h2 className="text-4xl font-bold text-white mb-12 text-center">Development Roadmap</h2>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
            {[
              {
                phase: "Phase 0",
                title: "The Seed",
                timeline: "Months 0-18",
                goals: ["ZK-rollup on Ethereum", "Self-sovereign identity", "UBC system", "10K users"],
                color: "from-purple-600 to-purple-700",
              },
              {
                phase: "Phase 1",
                title: "The Root",
                timeline: "Years 1-2",
                goals: ["Standalone L1", "Domain shards", "Physical anchoring", "1M TPS"],
                color: "from-pink-600 to-pink-700",
              },
              {
                phase: "Phase 2",
                title: "The Trunk",
                timeline: "Years 3-5",
                goals: ["Quantum resistance", "Hardware mesh", "Proof-of-useful-work", "Decentralized"],
                color: "from-red-600 to-red-700",
              },
              {
                phase: "Phase 3",
                title: "The Canopy",
                timeline: "Years 5-10",
                goals: ["Interplanetary", "Post-human governance", "AI agents", "1B+ users"],
                color: "from-orange-600 to-orange-700",
              },
            ].map((phase, i) => (
              <Card key={i} className="bg-gradient-to-br from-slate-900 to-slate-800 border-purple-500/20">
                <CardHeader>
                  <div className={`bg-gradient-to-r ${phase.color} rounded-lg px-3 py-1 inline-block mb-4 w-fit`}>
                    <span className="text-white text-xs font-bold">{phase.phase}</span>
                  </div>
                  <CardTitle className="text-white text-2xl">{phase.title}</CardTitle>
                  <CardDescription className="text-gray-400">{phase.timeline}</CardDescription>
                </CardHeader>
                <CardContent>
                  <ul className="space-y-2">
                    {phase.goals.map((goal, j) => (
                      <li key={j} className="flex items-start gap-2 text-gray-300">
                        <Check className="w-4 h-4 text-green-400 mt-0.5 flex-shrink-0" />
                        <span className="text-sm">{goal}</span>
                      </li>
                    ))}
                  </ul>
                </CardContent>
              </Card>
            ))}
          </div>
        </div>
      </section>

      {/* Use Cases Section */}
      <section className="py-20 px-4 sm:px-6 lg:px-8 border-t border-purple-500/10">
        <div className="max-w-7xl mx-auto">
          <h2 className="text-4xl font-bold text-white mb-12 text-center">Real-World Applications</h2>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
            {[
              { emoji: "💰", title: "Financial Inclusion", desc: "1.7B unbanked people can participate" },
              { emoji: "🌾", title: "Supply Chain", desc: "Complete provenance from source to consumer" },
              { emoji: "🏥", title: "Healthcare", desc: "Privacy-preserving medical records" },
              { emoji: "🎨", title: "Digital Rights", desc: "Artists keep 99% of revenue" },
              { emoji: "🤖", title: "Decentralized AI", desc: "Distributed training with shared ownership" },
              { emoji: "🚀", title: "Interplanetary Trade", desc: "Mars-Earth commerce without delays" },
            ].map((useCase, i) => (
              <Card key={i} className="bg-gradient-to-br from-purple-500/10 to-pink-500/10 border-purple-500/20 hover:border-purple-500/40 transition">
                <CardHeader>
                  <div className="text-4xl mb-4">{useCase.emoji}</div>
                  <CardTitle className="text-white">{useCase.title}</CardTitle>
                </CardHeader>
                <CardContent>
                  <p className="text-gray-300">{useCase.desc}</p>
                </CardContent>
              </Card>
            ))}
          </div>
        </div>
      </section>

      {/* CTA Section */}
      <section className="py-20 px-4 sm:px-6 lg:px-8 border-t border-purple-500/10">
        <div className="max-w-4xl mx-auto text-center">
          <h2 className="text-4xl font-bold text-white mb-6">Join the Future</h2>
          <p className="text-xl text-gray-300 mb-8">
            Omnia is open source and community-driven. Whether you're a cryptographer, developer, designer, or visionary, there's a place for you.
          </p>
          <div className="flex flex-col sm:flex-row gap-4 justify-center">
            <Button size="lg" className="bg-gradient-to-r from-purple-600 to-pink-600 hover:from-purple-700 hover:to-pink-700 text-white">
              Read Documentation
            </Button>
            <Button size="lg" variant="outline" className="border-purple-500/30 text-white hover:bg-purple-500/10">
              Contribute on GitHub
            </Button>
          </div>
        </div>
      </section>

      {/* Footer */}
      <footer className="py-12 px-4 sm:px-6 lg:px-8 border-t border-purple-500/10 bg-slate-950/50">
        <div className="max-w-7xl mx-auto">
          <div className="grid grid-cols-1 md:grid-cols-4 gap-8 mb-8">
            <div>
              <h3 className="text-white font-bold mb-4">Protocol</h3>
              <ul className="space-y-2 text-gray-400">
                <li><a href="#" className="hover:text-white transition">Architecture</a></li>
                <li><a href="#" className="hover:text-white transition">Specifications</a></li>
                <li><a href="#" className="hover:text-white transition">Roadmap</a></li>
              </ul>
            </div>
            <div>
              <h3 className="text-white font-bold mb-4">Community</h3>
              <ul className="space-y-2 text-gray-400">
                <li><a href="#" className="hover:text-white transition">GitHub</a></li>
                <li><a href="#" className="hover:text-white transition">Discord</a></li>
                <li><a href="#" className="hover:text-white transition">Forum</a></li>
              </ul>
            </div>
            <div>
              <h3 className="text-white font-bold mb-4">Resources</h3>
              <ul className="space-y-2 text-gray-400">
                <li><a href="#" className="hover:text-white transition">Documentation</a></li>
                <li><a href="#" className="hover:text-white transition">FAQ</a></li>
                <li><a href="#" className="hover:text-white transition">Use Cases</a></li>
              </ul>
            </div>
            <div>
              <h3 className="text-white font-bold mb-4">Legal</h3>
              <ul className="space-y-2 text-gray-400">
                <li><a href="#" className="hover:text-white transition">License (CC0)</a></li>
                <li><a href="#" className="hover:text-white transition">Code of Conduct</a></li>
                <li><a href="#" className="hover:text-white transition">Contributing</a></li>
              </ul>
            </div>
          </div>
          <div className="border-t border-purple-500/10 pt-8 text-center text-gray-400">
            <p>© 2026 Omnia Protocol. Public Domain (CC0). No entity owns this protocol.</p>
            <p className="mt-2 text-sm">Built for a future where trust is mathematically guaranteed.</p>
          </div>
        </div>
      </footer>
    </div>
  );
}
