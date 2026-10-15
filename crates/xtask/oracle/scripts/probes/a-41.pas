{ parity probe: a-41 }
unit P_a_41;

var
  Refs: array [0..1] of IInterface;
  RefsCount: integer;

function Initialize: integer;
begin
  RefsCount := 0;
end;

function Process(e: IInterface): integer;
begin
  Refs[RefsCount] := e;
  Inc(RefsCount);
end;

function Finalize: integer;
begin
  AddMessage('refs ' + IntToStr(RefsCount));
end;

end.
