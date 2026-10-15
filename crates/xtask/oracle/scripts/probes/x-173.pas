{ parity probe: x-173 }
unit P_x_173;

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
  v := Null; v := -v; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
