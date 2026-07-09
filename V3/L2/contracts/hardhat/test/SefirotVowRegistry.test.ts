import { expect } from "chai";
import { ethers, network } from "hardhat";
import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import { SefirotVowToken, SefirotVowRegistry } from "../typechain-types";

/** Fast-forward time by `seconds` using Hardhat evm helpers. */
async function fastForward(seconds: number) {
  await ethers.provider.send("evm_increaseTime", [seconds]);
  await ethers.provider.send("evm_mine", []);
}

describe("SefirotVowRegistry", () => {
  let token: SefirotVowToken;
  let registry: SefirotVowRegistry;
  let owner: SignerWithAddress;
  let sponsor1: SignerWithAddress;
  let sponsor2: SignerWithAddress;
  let candidate: SignerWithAddress;
  let other: SignerWithAddress;

  const VOW_HASH = ethers.id("I vow to care for the protocol as a Tree of Life");

  beforeEach(async () => {
    [owner, sponsor1, sponsor2, candidate, other] = await ethers.getSigners();
    // Deploy token with owner as minter (will be updated to registry)
    const TokenF = await ethers.getContractFactory("SefirotVowToken");
    token = await TokenF.deploy(owner.address);
    await token.waitForDeployment();

    // Deploy registry
    const RegistryF = await ethers.getContractFactory("SefirotVowRegistry");
    registry = await RegistryF.deploy(await token.getAddress());
    await registry.waitForDeployment();

    // Set registry as authorized minter
    await token.setAuthorizedMinter(await registry.getAddress());

    // Authorize sponsors
    await registry.setAuthorizedValidator(sponsor1.address, true);
    await registry.setAuthorizedValidator(sponsor2.address, true);
  });

  describe("Submit proposal", () => {
    it("submits with 2 sponsors", async () => {
      await expect(
        registry.connect(sponsor1).submitProposal(candidate.address, 0, VOW_HASH, sponsor2.address)
      ).to.emit(registry, "ProposalSubmitted");

      const p = await registry.getProposal(1);
      expect(p.candidate).to.equal(candidate.address);
      expect(p.sponsor1).to.equal(sponsor1.address);
      expect(p.sponsor2).to.equal(sponsor2.address);
      expect(p.state).to.equal(0n); // Pending
    });

    it("rejects unauthorized sponsor", async () => {
      await expect(
        registry.connect(other).submitProposal(candidate.address, 0, VOW_HASH, sponsor2.address)
      ).to.be.revertedWith("SefirotVowRegistry: sponsor1 not authorized");
    });

    it("rejects same sponsor twice", async () => {
      await expect(
        registry.connect(sponsor1).submitProposal(candidate.address, 0, VOW_HASH, sponsor1.address)
      ).to.be.revertedWith("SefirotVowRegistry: sponsors must differ");
    });
  });

  describe("Witness / Object", () => {
    beforeEach(async () => {
      await registry.connect(sponsor1).submitProposal(candidate.address, 1, VOW_HASH, sponsor2.address);
    });

    it("witnesses a proposal", async () => {
      await registry.connect(sponsor2).witness(1);
      const p = await registry.getProposal(1);
      expect(p.witnessCount).to.equal(1n);
    });

    it("rejects double witness", async () => {
      await registry.connect(sponsor2).witness(1);
      await expect(
        registry.connect(sponsor2).witness(1)
      ).to.be.revertedWith("SefirotVowRegistry: already witnessed");
    });

    it("objects and moves to Objection state", async () => {
      await expect(
        registry.connect(sponsor2).object(1, "insufficient residency")
      ).to.emit(registry, "Objected");
      const p = await registry.getProposal(1);
      expect(p.state).to.equal(2n); // Objection
    });
  });

  describe("Confirm", () => {
    beforeEach(async () => {
      await registry.connect(sponsor1).submitProposal(candidate.address, 2, VOW_HASH, sponsor2.address);
    });

    it("rejects confirm before review period ends", async () => {
      await expect(
        registry.confirm(1)
      ).to.be.revertedWith("SefirotVowRegistry: review period not ended");
    });

    it("confirms and mints token after review period", async () => {
      // Fast forward 7 days + 1 second
      await fastForward(7 * 24 * 60 * 60 + 1);

      await expect(registry.confirm(1))
        .to.emit(registry, "ProposalConfirmed");

      // Token minted
      expect(await token.hasActiveVow(candidate.address)).to.equal(true);
      // Candidate auto-authorized
      expect(await registry.isAuthorizedValidator(candidate.address)).to.equal(true);

      const p = await registry.getProposal(1);
      expect(p.state).to.equal(1n); // Confirmed
    });

    it("rejects confirm if objected", async () => {
      await registry.connect(sponsor2).object(1, "no");
      await fastForward(7 * 24 * 60 * 60 + 1);
      await expect(
        registry.confirm(1)
      ).to.be.revertedWith("SefirotVowRegistry: not pending");
    });
  });

  describe("Full lifecycle", () => {
    it("submit → witness → confirm → candidate becomes validator", async () => {
      // Submit
      await registry.connect(sponsor1).submitProposal(candidate.address, 0, VOW_HASH, sponsor2.address);
      // Witness
      await registry.connect(sponsor2).witness(1);
      // Fast forward
      await fastForward(7 * 24 * 60 * 60 + 1);
      // Confirm
      await registry.confirm(1);
      // Candidate now has vow + is authorized
      expect(await token.hasActiveVow(candidate.address)).to.equal(true);
      expect(await registry.isAuthorizedValidator(candidate.address)).to.equal(true);
    });
  });
});
