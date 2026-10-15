{ parity probe: x-189 }
unit P_x_189;

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
  i := not True; AddMessage(IntToStr(i));
  AddMessage('after');
end;
end.
