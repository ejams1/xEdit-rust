{ parity probe: a-18 }
unit P_a_18;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..5] of IInterface;
  i: Integer;
begin
  for i := 0 to 10 do
    a[i] := FileByIndex(0);
  AddMessage('after');
end;
end.
