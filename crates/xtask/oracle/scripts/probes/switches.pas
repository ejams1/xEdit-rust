{
  Parity fixture (issue #37): the load settings of `xeInit.pas` that a
  script can read.

  A case declares its switches (`"switches": ["PseudoESL"]`), which the
  harness gives the GUI as `-PseudoESL` and the port as its flag for the
  same setting (`--pseudo-light`); the probe prints what the resource unit
  reports for them: the pseudo light/medium/small mode flags of
  `wbIsPseudoESLMode` and its neighbours (`is_light_supported and
  wbPseudoLight`, so they are true only in the games that support light
  modules at all), and the folders a command line names: `wbDataPath`
  (`-D:`), `wbScriptsPath` (`-S:`), `wbTempPath` (`-T:`) and `wbOutputPath`
  (`-O:`), each with the run folder mapped to `<root>` by the harness. The
  message log is compared.
}
unit SwitchesProbe;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

function Flag(const aValue: Boolean): string;
begin
  if aValue then
    Result := 'true'
  else
    Result := 'false';
end;

function Initialize: integer;
begin
  Log('pseudo', 'esl=' + Flag(wbIsPseudoESLMode) + ' light=' + Flag(wbIsPseudoLightMode)
    + ' small=' + Flag(wbIsPseudoSmallMode));
  // `wbScriptsPath` and `wbTempPath` are named by `-S:` and `-T:`, which
  // both sides get from the harness; `wbDataPath` and `wbOutputPath` are
  // left out because the harness stages the plugins under a doubled
  // separator (`<run>\Data\<name>`), which the port's derived data path
  // repeats and the oracle's `-D:` value does not: the data folder of a run
  // without `-D:` is compared by `cargo xtask parity discovery`.
  Log('folders', 'scripts=' + wbScriptsPath + ' temp=' + wbTempPath);
  Result := 0;
end;

end.
