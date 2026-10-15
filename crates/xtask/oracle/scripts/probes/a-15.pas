{ parity probe: a-15 }
unit P_a_15;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array of Integer;
begin
  Show('Length', Length(a));
  Show('High', High(a));
  Show('Low', Low(a));
  SetLength(a, 0);
  Show('Length', Length(a));
end;
end.
