{ parity probe: x-025 }
unit P_x_025;

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
  s := IntToStr();
  AddMessage('after');
end;
end.
