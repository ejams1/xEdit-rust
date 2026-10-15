{ parity probe: x-136 }
unit P_x_136;

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
  AddMessage(IntToStr('5' * 2));
  AddMessage('after');
end;
end.
