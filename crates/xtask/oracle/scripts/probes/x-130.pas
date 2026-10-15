{ parity probe: x-130 }
unit P_x_130;

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
  AddMessage(IntToStr(Pos('a', 5)));
  AddMessage('after');
end;
end.
