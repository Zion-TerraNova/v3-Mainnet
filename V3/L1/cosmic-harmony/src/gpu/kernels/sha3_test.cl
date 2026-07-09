#pragma OPENCL EXTENSION cl_amd_printf : enable

__constant ulong RC[24] = {
    0x0000000000000001UL, 0x0000000000008082UL,
    0x800000000000808AUL, 0x8000000080008000UL,
    0x000000000000808BUL, 0x0000000080000001UL,
    0x8000000080008081UL, 0x8000000000008009UL,
    0x000000000000008AUL, 0x0000000000000088UL,
    0x0000000080008009UL, 0x000000008000000AUL,
    0x000000008000808BUL, 0x800000000000008BUL,
    0x8000000000008089UL, 0x8000000000008003UL,
    0x8000000000008002UL, 0x8000000000000080UL,
    0x000000000000800AUL, 0x800000008000000AUL,
    0x8000000080008081UL, 0x8000000000008080UL,
    0x0000000080000001UL, 0x8000000080008008UL
};

#define ROL(a, offset) ((((ulong)a) << ((offset) & 63)) ^ (((ulong)a) >> ((64 - (offset)) & 63)))

void keccak_f1600(ulong *st)
{
    int round;
    ulong t0, t1;
    ulong Aba, Abe, Abi, Abo, Abu;
    ulong Aga, Age, Agi, Ago, Agu;
    ulong Aka, Ake, Aki, Ako, Aku;
    ulong Ama, Ame, Ami, Amo, Amu;
    ulong Asa, Ase, Asi, Aso, Asu;

    Aba = st[0];  Abe = st[1];  Abi = st[2];  Abo = st[3];  Abu = st[4];
    Aga = st[5];  Age = st[6];  Agi = st[7];  Ago = st[8];  Agu = st[9];
    Aka = st[10]; Ake = st[11]; Aki = st[12]; Ako = st[13]; Aku = st[14];
    Ama = st[15]; Ame = st[16]; Ami = st[17]; Amo = st[18]; Amu = st[19];
    Asa = st[20]; Ase = st[21]; Asi = st[22]; Aso = st[23]; Asu = st[24];

    for (round = 0; round < 24; round += 2) {
        t0 = Aba ^ Aga ^ Aka ^ Ama ^ Asa;
        t1 = Abe ^ Age ^ Ake ^ Ame ^ Ase;
        ulong t2 = Abi ^ Agi ^ Aki ^ Ami ^ Asi;
        ulong t3 = Abo ^ Ago ^ Ako ^ Amo ^ Aso;
        ulong t4 = Abu ^ Agu ^ Aku ^ Amu ^ Asu;

        ulong Da = ROL(t1, 1) ^ t4;
        ulong De = ROL(t2, 1) ^ t0;
        ulong Di = ROL(t3, 1) ^ t1;
        ulong Do = ROL(t4, 1) ^ t2;
        ulong Du = ROL(t0, 1) ^ t3;

        Aba ^= Da; Abe ^= De; Abi ^= Di; Abo ^= Do; Abu ^= Du;
        Aga ^= Da; Age ^= De; Agi ^= Di; Ago ^= Do; Agu ^= Du;
        Aka ^= Da; Ake ^= De; Aki ^= Di; Ako ^= Do; Aku ^= Du;
        Ama ^= Da; Ame ^= De; Ami ^= Di; Amo ^= Do; Amu ^= Du;
        Asa ^= Da; Ase ^= De; Asi ^= Di; Aso ^= Do; Asu ^= Du;

        t0 = Aba; t1 = Abe;
        Aba = (t0 ^ ((~Abe) & Abi)) ^ RC[round];
        Abe = (t1 ^ ((~Abi) & Abo));
        Abi = (Abi ^ ((~Abo) & Abu));
        Abo = (Abo ^ ((~Abu) & t0));
        Abu = (Abu ^ ((~t0) & t1));

        t0 = Aga; t1 = Age;
        Aga = (t0 ^ ((~Age) & Agi));
        Age = (Age ^ ((~Agi) & Ago));
        Agi = (Agi ^ ((~Ago) & Agu));
        Ago = (Ago ^ ((~Agu) & t0));
        Agu = (Agu ^ ((~t0) & t1));

        t0 = Aka; t1 = Ake;
        Aka = (t0 ^ ((~Ake) & Aki));
        Ake = (Ake ^ ((~Aki) & Ako));
        Aki = (Aki ^ ((~Ako) & Aku));
        Ako = (Ako ^ ((~Aku) & t0));
        Aku = (Aku ^ ((~t0) & t1));

        t0 = Ama; t1 = Ame;
        Ama = (t0 ^ ((~Ame) & Ami));
        Ame = (Ame ^ ((~Ami) & Amo));
        Ami = (Ami ^ ((~Amo) & Amu));
        Amo = (Amo ^ ((~Amu) & t0));
        Amu = (Amu ^ ((~t0) & t1));

        t0 = Asa; t1 = Ase;
        Asa = (t0 ^ ((~Ase) & Asi));
        Ase = (Ase ^ ((~Asi) & Aso));
        Asi = (Asi ^ ((~Aso) & Asu));
        Aso = (Aso ^ ((~Asu) & t0));
        Asu = (Asu ^ ((~t0) & t1));

        t0 = Aba;     t1 = Abe;     Aba = ROL(Aba,  0); Abe = ROL(Abe,  1);
        Abi = ROL(Abi, 62); Abo = ROL(Abo, 28); Abu = ROL(Abu, 27);
        Aga = ROL(Aga, 36); Age = ROL(Age, 44); Agi = ROL(Agi,  6); Ago = ROL(Ago, 55); Agu = ROL(Agu, 20);
        Aka = ROL(Aka,  3); Ake = ROL(Ake, 10); Aki = ROL(Aki, 43); Ako = ROL(Ako, 25); Aku = ROL(Aku, 39);
        Ama = ROL(Ama, 41); Ame = ROL(Ame, 45); Ami = ROL(Ami, 15); Amo = ROL(Amo, 21); Amu = ROL(Amu,  8);
        Asa = ROL(Asa, 18); Ase = ROL(Ase,  2); Asi = ROL(Asi, 61); Aso = ROL(Aso, 56); Asu = ROL(Asu, 14);

        t0 = Aba ^ Aga ^ Aka ^ Ama ^ Asa;
        t1 = Abe ^ Age ^ Ake ^ Ame ^ Ase;
        t2 = Abi ^ Agi ^ Aki ^ Ami ^ Asi;
        t3 = Abo ^ Ago ^ Ako ^ Amo ^ Aso;
        t4 = Abu ^ Agu ^ Aku ^ Amu ^ Asu;

        Da = ROL(t1, 1) ^ t4;
        De = ROL(t2, 1) ^ t0;
        Di = ROL(t3, 1) ^ t1;
        Do = ROL(t4, 1) ^ t2;
        Du = ROL(t0, 1) ^ t3;

        Aba ^= Da; Abe ^= De; Abi ^= Di; Abo ^= Do; Abu ^= Du;
        Aga ^= Da; Age ^= De; Agi ^= Di; Ago ^= Do; Agu ^= Du;
        Aka ^= Da; Ake ^= De; Aki ^= Di; Ako ^= Do; Aku ^= Du;
        Ama ^= Da; Ame ^= De; Ami ^= Di; Amo ^= Do; Amu ^= Du;
        Asa ^= Da; Ase ^= De; Asi ^= Di; Aso ^= Do; Asu ^= Du;

        t0 = Aba; t1 = Abe;
        Aba = (t0 ^ ((~Abe) & Abi)) ^ RC[round + 1];
        Abe = (t1 ^ ((~Abi) & Abo));
        Abi = (Abi ^ ((~Abo) & Abu));
        Abo = (Abo ^ ((~Abu) & t0));
        Abu = (Abu ^ ((~t0) & t1));

        t0 = Aga; t1 = Age;
        Aga = (t0 ^ ((~Age) & Agi));
        Age = (Age ^ ((~Agi) & Ago));
        Agi = (Agi ^ ((~Ago) & Agu));
        Ago = (Ago ^ ((~Agu) & t0));
        Agu = (Agu ^ ((~t0) & t1));

        t0 = Aka; t1 = Ake;
        Aka = (t0 ^ ((~Ake) & Aki));
        Ake = (Ake ^ ((~Aki) & Ako));
        Aki = (Aki ^ ((~Ako) & Aku));
        Ako = (Ako ^ ((~Aku) & t0));
        Aku = (Aku ^ ((~t0) & t1));

        t0 = Ama; t1 = Ame;
        Ama = (t0 ^ ((~Ame) & Ami));
        Ame = (Ame ^ ((~Ami) & Amo));
        Ami = (Ami ^ ((~Amo) & Amu));
        Amo = (Amo ^ ((~Amu) & t0));
        Amu = (Amu ^ ((~t0) & t1));

        t0 = Asa; t1 = Ase;
        Asa = (t0 ^ ((~Ase) & Asi));
        Ase = (Ase ^ ((~Asi) & Aso));
        Asi = (Asi ^ ((~Aso) & Asu));
        Aso = (Aso ^ ((~Asu) & t0));
        Asu = (Asu ^ ((~t0) & t1));

        Aba = ROL(Aba,  0); Abe = ROL(Abe,  1); Abi = ROL(Abi, 62); Abo = ROL(Abo, 28); Abu = ROL(Abu, 27);
        Aga = ROL(Aga, 36); Age = ROL(Age, 44); Agi = ROL(Agi,  6); Ago = ROL(Ago, 55); Agu = ROL(Agu, 20);
        Aka = ROL(Aka,  3); Ake = ROL(Ake, 10); Aki = ROL(Aki, 43); Ako = ROL(Ako, 25); Aku = ROL(Aku, 39);
        Ama = ROL(Ama, 41); Ame = ROL(Ame, 45); Ami = ROL(Ami, 15); Amo = ROL(Amo, 21); Amu = ROL(Amu,  8);
        Asa = ROL(Asa, 18); Ase = ROL(Ase,  2); Asi = ROL(Asi, 61); Aso = ROL(Aso, 56); Asu = ROL(Asu, 14);
    }

    st[0]  = Aba;  st[1]  = Abe;  st[2]  = Abi;  st[3]  = Abo;  st[4]  = Abu;
    st[5]  = Aga;  st[6]  = Age;  st[7]  = Agi;  st[8]  = Ago;  st[9]  = Agu;
    st[10] = Aka;  st[11] = Ake;  st[12] = Aki;  st[13] = Ako;  st[14] = Aku;
    st[15] = Ama;  st[16] = Ame;  st[17] = Ami;  st[18] = Amo;  st[19] = Amu;
    st[20] = Asa;  st[21] = Ase;  st[22] = Asi;  st[23] = Aso;  st[24] = Asu;
}

void sha3_512_test(const uchar *in, int inlen, uchar *out)
{
    ulong st[25];
    for (int i = 0; i < 25; i++) st[i] = 0;
    int pos = 0;

    while (inlen > 0) {
        int chunk = 72 - pos;
        if (chunk > inlen) chunk = inlen;
        for (int i = 0; i < chunk; i++)
            ((uchar *)st)[pos + i] ^= in[i];
        pos += chunk;
        inlen -= chunk;
        in += chunk;
        if (pos == 72) {
            keccak_f1600(st);
            pos = 0;
        }
    }

    ((uchar *)st)[pos] ^= 0x06;
    ((uchar *)st)[71] ^= 0x80;
    keccak_f1600(st);

    for (int i = 0; i < 8; i++) out[i] = ((uchar *)st)[i];
}

__kernel void sha3_test(__global uchar *output)
{
    uchar in[5] = {'h','e','l','l','o'};
    uchar out[8];
    sha3_512_test(in, 5, out);
    for (int i = 0; i < 8; i++)
        output[i] = out[i];
}
