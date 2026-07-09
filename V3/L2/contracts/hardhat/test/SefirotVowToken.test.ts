import { expect } from "chai";
import { ethers } from "hardhat";
import { SignerWithAddress } from "@nomicfoundation/hardhat-ethers/signers";
import { SefirotVowToken } from "../typechain-types";

describe("SefirotVowToken", () => {
  let token: SefirotVowToken;
  let owner: SignerWithAddress;
  let minter: SignerWithAddress;
  let validator: SignerWithAddress;
  let other: SignerWithAddress;

  const VOW_HASH = ethers.id("I vow to care for the protocol as a Tree of Life");
  const VOW_HASH_2 = ethers.id("I renew my vow to care for the protocol");

  beforeEach(async () => {
    [owner, minter, validator, other] = await ethers.getSigners();
    const Factory = await ethers.getContractFactory("SefirotVowToken");
    token = await Factory.deploy(minter.address);
    await token.waitForDeployment();
  });

  describe("Deployment", () => {
    it("sets name and symbol", async () => {
      expect(await token.name()).to.equal("ZION Sefirot Vow");
      expect(await token.symbol()).to.equal("SEFIROT-VOW");
    });

    it("sets authorized minter", async () => {
      expect(await token.authorizedMinter()).to.equal(minter.address);
    });
  });

  describe("Mint", () => {
    it("mints soulbound token to validator", async () => {
      await token.connect(minter).mint(validator.address, 0, VOW_HASH);
      const tokenId = await token.validatorToTokenId(validator.address);
      expect(tokenId).to.equal(1n);
      expect(await token.ownerOf(1)).to.equal(validator.address);

      const vow = await token.getVow(validator.address);
      expect(vow.validator).to.equal(validator.address);
      expect(vow.validatorClass).to.equal(0n); // L1Miner
      expect(vow.vowHash).to.equal(VOW_HASH);
      expect(vow.state).to.equal(0n); // Active
    });

    it("rejects mint from unauthorized", async () => {
      await expect(
        token.connect(other).mint(validator.address, 0, VOW_HASH)
      ).to.be.revertedWith("SefirotVow: not authorized");
    });

    it("rejects double mint", async () => {
      await token.connect(minter).mint(validator.address, 0, VOW_HASH);
      await expect(
        token.connect(minter).mint(validator.address, 0, VOW_HASH)
      ).to.be.revertedWith("SefirotVow: already has vow");
    });

    it("rejects zero address", async () => {
      await expect(
        token.connect(minter).mint(ethers.ZeroAddress, 0, VOW_HASH)
      ).to.be.revertedWith("SefirotVow: zero address");
    });

    it("rejects invalid class", async () => {
      await expect(
        token.connect(minter).mint(validator.address, 5, VOW_HASH)
      ).to.be.revertedWith("SefirotVow: invalid class");
    });
  });

  describe("Soulbound — non-transferable", () => {
    beforeEach(async () => {
      await token.connect(minter).mint(validator.address, 0, VOW_HASH);
    });

    it("blocks transfer", async () => {
      await expect(
        token.connect(validator).transferFrom(validator.address, other.address, 1)
      ).to.be.revertedWith("SefirotVow: soulbound - non-transferable");
    });

    it("blocks safeTransferFrom", async () => {
      await expect(
        token.connect(validator)["safeTransferFrom(address,address,uint256)"](
          validator.address, other.address, 1
        )
      ).to.be.revertedWith("SefirotVow: soulbound - non-transferable");
    });
  });

  describe("Renew", () => {
    beforeEach(async () => {
      await token.connect(minter).mint(validator.address, 1, VOW_HASH);
    });

    it("renews vow hash", async () => {
      await token.connect(validator).renew(VOW_HASH_2);
      const vow = await token.getVow(validator.address);
      expect(vow.vowHash).to.equal(VOW_HASH_2);
      expect(vow.lastRenewedAt).to.be.gt(0n);
    });

    it("clears suspension on renewal", async () => {
      await token.connect(minter).suspend(validator.address, "test break");
      let vow = await token.getVow(validator.address);
      expect(vow.state).to.equal(1n); // Suspended

      await token.connect(validator).renew(VOW_HASH_2);
      vow = await token.getVow(validator.address);
      expect(vow.state).to.equal(0n); // Active
    });

    it("rejects renew from non-validator", async () => {
      await expect(
        token.connect(other).renew(VOW_HASH_2)
      ).to.be.revertedWith("SefirotVow: no vow");
    });
  });

  describe("Suspend", () => {
    beforeEach(async () => {
      await token.connect(minter).mint(validator.address, 2, VOW_HASH);
    });

    it("suspends active vow", async () => {
      await token.connect(minter).suspend(validator.address, "first break");
      const vow = await token.getVow(validator.address);
      expect(vow.state).to.equal(1n); // Suspended
      expect(vow.suspensionCount).to.equal(1n);
    });

    it("auto-revokes after 3 suspensions", async () => {
      await token.connect(minter).suspend(validator.address, "break 1");
      await token.connect(minter).suspend(validator.address, "break 2");
      await expect(
        token.connect(minter).suspend(validator.address, "break 3")
      ).to.emit(token, "VowRevoked");

      const tokenId = await token.validatorToTokenId(validator.address);
      expect(tokenId).to.equal(0n); // unlinked
      expect(await token.revokedAt(validator.address)).to.be.gt(0n);
    });

    it("rejects suspend from unauthorized", async () => {
      await expect(
        token.connect(other).suspend(validator.address, "nope")
      ).to.be.revertedWith("SefirotVow: not authorized");
    });
  });

  describe("Revoke", () => {
    beforeEach(async () => {
      await token.connect(minter).mint(validator.address, 3, VOW_HASH);
    });

    it("revokes and burns token", async () => {
      await token.connect(minter).revoke(validator.address, "severe violation");
      const tokenId = await token.validatorToTokenId(validator.address);
      expect(tokenId).to.equal(0n);
      expect(await token.revokedAt(validator.address)).to.be.gt(0n);
      await expect(token.ownerOf(1)).to.be.reverted; // burned
    });

    it("rejects re-mint during cooldown", async () => {
      await token.connect(minter).revoke(validator.address, "violation");
      await expect(
        token.connect(minter).mint(validator.address, 3, VOW_HASH)
      ).to.be.revertedWith("SefirotVow: revocation cooldown active");
    });
  });

  describe("View functions", () => {
    beforeEach(async () => {
      await token.connect(minter).mint(validator.address, 0, VOW_HASH);
    });

    it("hasActiveVow returns true for active", async () => {
      expect(await token.hasActiveVow(validator.address)).to.equal(true);
      expect(await token.hasActiveVow(other.address)).to.equal(false);
    });

    it("totalVowsMinted counts correctly", async () => {
      expect(await token.totalVowsMinted()).to.equal(1n);
      await token.connect(minter).mint(other.address, 0, VOW_HASH);
      expect(await token.totalVowsMinted()).to.equal(2n);
    });
  });
});
