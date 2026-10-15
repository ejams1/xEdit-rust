{ parity probe: a-21 }
unit P_a_21;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  s: string;
begin
  s := 'abc';
  SetLength(s, 2);
  Show('s', s);
  SetLength(s, 4);
  Show('Length', Length(s));
  Show('Ord(s[4])', Ord(s[4]));
end;
end.
