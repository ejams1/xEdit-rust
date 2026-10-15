{ parity probe: a-19 }
unit P_a_19;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array of Integer;
begin
  SetLength(a, -1);
  Show('Length', Length(a));
end;
end.
