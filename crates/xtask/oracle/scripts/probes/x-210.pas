{ parity probe: x-210 }
unit P_x_210;

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
  v := sl = 5; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
