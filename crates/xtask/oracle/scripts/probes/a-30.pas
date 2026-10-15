{ parity probe: a-30 }
unit P_a_30;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array of Integer;
begin
  SetLength(a, 2);
  a[0] := 1;
  a[5] := 2;
  AddMessage('after');
end;
end.
