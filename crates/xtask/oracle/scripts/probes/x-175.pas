{ parity probe: x-175 }
unit P_x_175;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := Null; v := v and True; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
